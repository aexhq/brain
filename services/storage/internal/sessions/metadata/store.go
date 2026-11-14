// Package metadata stores session heads on disk and publishes them only after
// all referenced transcript, attachment and native-state objects are committed.
package metadata

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"hash/fnv"
	"sort"
	"sync"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aexhq/brain/services/storage/internal/tiering"
)

type stored struct {
	Record         api.Record `json:"record"`
	RemoteVersion  string     `json:"remote_version,omitempty"`
	SyncedRevision uint64     `json:"synced_revision"`
}
type Store struct {
	Disk, Remote adapters.Adapter
	Objects      *tiering.Store
	locks        [64]sync.Mutex
	flush        [64]sync.Mutex
}

func Namespace(tenant string) string {
	b := sha256.Sum256([]byte(tenant))
	return hex.EncodeToString(b[:])
}
func key(i api.Identity) string {
	// Hex preserves caller-ID ordering. Split long components to keep every
	// backend's path limits; the terminator orders an ID before its extensions.
	id := hex.EncodeToString([]byte(i.Session))
	if len(id) > 128 {
		id = id[:128] + "/" + id[128:]
	}
	return Namespace(i.Tenant) + "/sessions/" + id + "!"
}
func slot(key string) uint32 { h := fnv.New32a(); _, _ = h.Write([]byte(key)); return h.Sum32() % 64 }
func valid(i api.Identity) error {
	if i.Tenant == "" || len(i.Tenant) > 128 || !core.ValidSessionID(i.Session) || (i.Collection != "" && i.Collection != "packages") {
		return core.E(core.Invalid, "tenant and session are required")
	}
	return nil
}
func (s *Store) read(ctx context.Context, k string) (stored, error) {
	var v stored
	o, err := s.Disk.Get(ctx, k)
	if errors.Is(err, adapters.ErrNotFound) && s.Remote != nil {
		o, err = s.Remote.Get(ctx, k)
		if err == nil {
			if err = json.Unmarshal(o.Data, &v.Record); err != nil {
				return v, err
			}
			v.RemoteVersion = o.Version
			v.SyncedRevision = v.Record.Revision
			err = s.write(ctx, k, v)
			return v, err
		}
	}
	if err != nil {
		return v, err
	}
	err = json.Unmarshal(o.Data, &v)
	if err == nil && v.Record.Revision == v.SyncedRevision {
		if cache, ok := s.Disk.(*tiering.Store); ok {
			err = cache.MarkCommitted(ctx, k, o.Version)
		}
	}
	return v, err
}
func (s *Store) write(ctx context.Context, k string, v stored) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	o, err := s.Disk.Put(ctx, k, b, adapters.Condition{})
	if err == nil && v.Record.Revision == v.SyncedRevision {
		if cache, ok := s.Disk.(*tiering.Store); ok {
			err = cache.MarkCommitted(ctx, k, o.Version)
		}
	}
	return err
}
func (s *Store) Get(ctx context.Context, i api.Identity) (api.Record, error) {
	if err := valid(i); err != nil {
		return api.Record{}, err
	}
	k := key(i)
	m := &s.locks[slot(k)]
	m.Lock()
	defer m.Unlock()
	v, err := s.read(ctx, k)
	return v.Record, err
}

func (s *Store) Status(ctx context.Context, i api.Identity) (core.SyncStatus, error) {
	status := core.SyncStatus{Pending: true}
	if err := valid(i); err != nil {
		return status, err
	}
	k := key(i)
	m := &s.locks[slot(k)]
	m.Lock()
	defer m.Unlock()
	v, err := s.read(ctx, k)
	if err != nil {
		return status, err
	}
	if v.SyncedRevision == 0 {
		return status, nil
	}
	if s.Remote == nil {
		return status, core.E(core.Unsupported, "external metadata is not configured")
	}
	o, err := s.Remote.Get(ctx, k)
	if err != nil {
		return status, err
	}
	var r api.Record
	if err = json.Unmarshal(o.Data, &r); err != nil {
		return status, err
	}
	status.Pending = r.Revision != v.Record.Revision
	status.Checkpoint, status.HistoryBytes = r.Snapshot, r.TranscriptBytes
	if cp := r.Session.Checkpoint; cp != nil {
		status.ResumeOffset, status.NativeCheckpoint, status.Time = cp.Offset, cp.ID, cp.Time
	}
	return status, nil
}
func owned(v api.Record, i api.Identity) error {
	if i.Owner == "" || i.Epoch == 0 || v.Owner != i.Owner || v.Epoch != i.Epoch {
		return core.E(core.Conflict, "session owner was fenced")
	}
	if v.Deleted {
		return core.E(core.NotFound, "session was deleted")
	}
	return nil
}
func (s *Store) Put(ctx context.Context, i api.Identity, r api.Record, expected uint64) (api.Record, error) {
	if err := valid(i); err != nil {
		return r, err
	}
	if r.Tenant != i.Tenant || r.Session.ID != i.Session || i.Owner == "" || i.Epoch == 0 {
		return r, core.E(core.Invalid, "inconsistent session identity")
	}
	k := key(i)
	m := &s.locks[slot(k)]
	m.Lock()
	defer m.Unlock()
	v, err := s.read(ctx, k)
	if errors.Is(err, adapters.ErrNotFound) && expected == 0 {
		v = stored{}
		err = nil
	} else if err == nil {
		if expected == 0 {
			return r, core.E(core.Conflict, "session already exists")
		}
		if err = owned(v.Record, i); err != nil {
			return r, err
		}
	}
	if err != nil {
		return r, err
	}
	if v.Record.Revision != expected {
		return r, core.E(core.Conflict, "session revision changed")
	}
	r.Owner = i.Owner
	r.Epoch = i.Epoch
	r.Revision = expected + 1
	r.Session.Tenant = i.Tenant
	r.Session.Credentials = r.Credentials
	v.Record = r
	return r, s.write(ctx, k, v)
}

// Claim accepts a fencing epoch supplied by placement. It never invents an
// ownership lease or derives ownership from asynchronously replicated metadata.
func (s *Store) Claim(ctx context.Context, i api.Identity) (api.Record, error) {
	if err := valid(i); err != nil {
		return api.Record{}, err
	}
	k := key(i)
	m := &s.locks[slot(k)]
	m.Lock()
	defer m.Unlock()
	v, err := s.read(ctx, k)
	if err != nil {
		return v.Record, err
	}
	if i.Owner == "" || i.Epoch <= v.Record.Epoch || v.Record.Deleted {
		return v.Record, core.E(core.Conflict, "a newer ownership epoch is required")
	}
	v.Record.Owner = i.Owner
	v.Record.Epoch = i.Epoch
	v.Record.Revision++
	return v.Record, s.write(ctx, k, v)
}
func references(r api.Record) []string {
	refs := []string{}
	for _, v := range []string{r.Transcript, r.Snapshot, r.Attachments} {
		if v != "" {
			refs = append(refs, v)
		}
	}
	return refs
}
func ObjectKey(i api.Identity, ref string) (string, error) {
	if len(ref) != 64 {
		return "", core.E(core.Invalid, "invalid object reference")
	}
	if _, err := hex.DecodeString(ref); err != nil {
		return "", core.E(core.Invalid, "invalid object reference")
	}
	collection := "objects"
	if i.Collection == "packages" {
		collection = "packages"
	}
	return Namespace(i.Tenant) + "/" + collection + "/" + core.SessionDirectory(core.SessionKey(i.Tenant, i.Session)) + "/" + ref, nil
}
func (s *Store) PutObject(ctx context.Context, i api.Identity, o api.Object) (string, error) {
	if err := valid(i); err != nil {
		return "", err
	}
	m := &s.locks[slot(key(i))]
	m.Lock()
	defer m.Unlock()
	state, err := s.read(ctx, key(i))
	if err != nil && !errors.Is(err, adapters.ErrNotFound) {
		return "", err
	}
	if err == nil && state.Record.Deleted && i.Collection == "" {
		return "", core.E(core.NotFound, "session was deleted")
	}
	// Uploaded code and clients cannot grow one object without bound. Larger
	// artifacts are streamed as chunks; dependencies stay within the tenant.
	if len(o.Data) > api.ChunkBytes || len(o.References) > 1024 {
		return "", core.E(core.Invalid, "object exceeds chunk or reference limit")
	}
	for _, r := range o.References {
		if _, err := s.GetObject(ctx, i, r); err != nil {
			return "", err
		}
	}
	b, err := json.Marshal(o)
	if err != nil {
		return "", err
	}
	h := sha256.Sum256(b)
	ref := hex.EncodeToString(h[:])
	k, _ := ObjectKey(i, ref)
	if _, err = s.Objects.Get(ctx, k); err == nil {
		return ref, nil
	} else if !errors.Is(err, adapters.ErrNotFound) {
		return "", err
	}
	_, err = s.Objects.Put(ctx, k, b, adapters.Condition{CreateOnly: true})
	if errors.Is(err, adapters.ErrConflict) {
		err = nil
	}
	return ref, err
}

// Erase follows an externally durable tombstone. Object namespaces are scoped
// to sessions, so deleting one cannot remove another session's identical data.
func (s *Store) Erase(ctx context.Context, i api.Identity) error {
	r, err := s.Get(ctx, i)
	if err != nil {
		return err
	}
	if !r.Deleted {
		return core.E(core.Conflict, "close and tombstone the session before erasing it")
	}
	if _, err = s.Flush(ctx, i); err != nil {
		return err
	}
	m := &s.locks[slot(key(i))]
	m.Lock()
	defer m.Unlock()
	prefix := Namespace(i.Tenant) + "/objects/" + core.SessionDirectory(core.SessionKey(i.Tenant, i.Session)) + "/"
	return s.Objects.DeletePrefix(ctx, prefix)
}
func (s *Store) GetObject(ctx context.Context, i api.Identity, ref string) (api.Object, error) {
	var v api.Object
	if err := valid(i); err != nil {
		return v, err
	}
	k, err := ObjectKey(i, ref)
	if err != nil {
		return v, err
	}
	o, err := s.Objects.Get(ctx, k)
	if err != nil {
		return v, err
	}
	h := sha256.Sum256(o.Data)
	if hex.EncodeToString(h[:]) != ref {
		return v, errors.New("corrupt immutable object")
	}
	err = json.Unmarshal(o.Data, &v)
	return v, err
}

func (s *Store) commitTree(ctx context.Context, i api.Identity, ref string) error {
	type visit struct {
		reference string
		expanded  bool
	}
	stack := []visit{{reference: ref}}
	for len(stack) != 0 {
		v := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		k, err := ObjectKey(i, v.reference)
		if err != nil {
			return err
		}
		if v.expanded {
			if err = s.Objects.Commit(ctx, k); err != nil {
				return err
			}
			continue
		}
		committed, err := s.Objects.Committed(ctx, k)
		if err != nil {
			return err
		}
		if committed {
			continue
		}
		o, err := s.GetObject(ctx, i, v.reference)
		if err != nil {
			return err
		}
		stack = append(stack, visit{reference: v.reference, expanded: true})
		for _, child := range o.References {
			stack = append(stack, visit{reference: child})
		}
	}
	return nil
}

func (s *Store) CommitObject(ctx context.Context, i api.Identity, ref string) error {
	if err := valid(i); err != nil {
		return err
	}
	return s.commitTree(ctx, i, ref)
}
func (s *Store) Flush(ctx context.Context, i api.Identity) (api.Record, error) {
	if s.Remote == nil {
		return api.Record{}, core.E(core.Unsupported, "external metadata is not configured")
	}
	if err := valid(i); err != nil {
		return api.Record{}, err
	}
	k := key(i)
	serial := &s.flush[slot(k)]
	serial.Lock()
	defer serial.Unlock()
	m := &s.locks[slot(k)]
	m.Lock()
	v, err := s.read(ctx, k)
	m.Unlock()
	if err != nil {
		return v.Record, err
	}
	if v.SyncedRevision == v.Record.Revision {
		return v.Record, nil
	}
	for _, ref := range references(v.Record) {
		if err = s.commitTree(ctx, i, ref); err != nil {
			return v.Record, err
		}
	}
	b, err := json.Marshal(v.Record)
	if err != nil {
		return v.Record, err
	}
	c := adapters.Condition{Version: v.RemoteVersion}
	if c.Version == "" {
		c.CreateOnly = true
	}
	remote, err := s.Remote.Put(ctx, k, b, c)
	if errors.Is(err, adapters.ErrConflict) {
		// A previous publish may have committed before its acknowledgement or
		// local version update was lost. Recover only our own monotonic history.
		known, readErr := s.Remote.Get(ctx, k)
		var head api.Record
		if readErr == nil && json.Unmarshal(known.Data, &head) == nil && head.Owner == v.Record.Owner && head.Epoch == v.Record.Epoch && head.Revision > v.SyncedRevision && head.Revision <= v.Record.Revision {
			if head.Revision == v.Record.Revision && bytes.Equal(known.Data, b) {
				remote, err = known, nil
			} else if head.Revision < v.Record.Revision {
				remote, err = s.Remote.Put(ctx, k, b, adapters.Condition{Version: known.Version})
			}
		}
	}
	if err != nil {
		return v.Record, err
	}
	m.Lock()
	defer m.Unlock()
	current, err := s.read(ctx, k)
	if err != nil {
		return v.Record, err
	}
	current.RemoteVersion = remote.Version
	current.SyncedRevision = v.Record.Revision
	return v.Record, s.write(ctx, k, current)
}
func (s *Store) List(ctx context.Context, tenant, after string, limit int) ([]api.Record, string, error) {
	if tenant == "" {
		return nil, "", core.E(core.Invalid, "tenant is required")
	}
	if limit < 1 || limit > 1000 {
		return nil, "", core.E(core.Invalid, "invalid list limit")
	}
	prefix := Namespace(tenant) + "/sessions/"
	if after != "" {
		if !core.ValidSessionID(after) {
			return nil, "", core.E(core.Invalid, "invalid session cursor")
		}
		after = key(api.Identity{Tenant: tenant, Session: after})
	}
	p, err := s.Disk.List(ctx, prefix, after, limit)
	if err != nil {
		return nil, "", err
	}
	keys := map[string]bool{}
	for _, o := range p.Objects {
		keys[o.Key] = true
	}
	more := p.Next != ""
	if s.Remote != nil {
		remote, e := s.Remote.List(ctx, prefix, after, limit)
		if e != nil {
			return nil, "", e
		}
		more = more || remote.Next != ""
		for _, o := range remote.Objects {
			keys[o.Key] = true
		}
	}
	ordered := make([]string, 0, len(keys))
	for k := range keys {
		ordered = append(ordered, k)
	}
	sort.Strings(ordered)
	if len(ordered) > limit {
		more = true
		ordered = ordered[:limit]
	}
	result := make([]api.Record, 0, len(ordered))
	bytes := 0
	next := ""
	for _, k := range ordered {
		m := &s.locks[slot(k)]
		m.Lock()
		v, e := s.read(ctx, k)
		m.Unlock()
		if e != nil {
			return nil, "", e
		}
		b, e := json.Marshal(v.Record)
		if e != nil {
			return nil, "", e
		}
		if bytes+len(b) > 1<<20 && len(result) > 0 {
			more = true
			break
		}
		bytes += len(b)
		result = append(result, v.Record)
		next = v.Record.Session.ID
	}
	if !more {
		next = ""
	}
	return result, next, nil
}
func (s *Store) Replicate(ctx context.Context, interval time.Duration, onError func(error)) {
	t := time.NewTicker(interval)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
		}
		if cache, ok := s.Disk.(*tiering.Store); ok {
			keys, err := cache.DirtyKeys(ctx)
			if err != nil {
				onError(err)
				continue
			}
			for _, k := range keys {
				m := &s.locks[slot(k)]
				m.Lock()
				v, e := s.read(ctx, k)
				m.Unlock()
				if e != nil {
					onError(e)
					continue
				}
				c, cancel := context.WithTimeout(ctx, 45*time.Second)
				_, e = s.Flush(c, api.Identity{Tenant: v.Record.Tenant, Session: v.Record.Session.ID})
				cancel()
				if e != nil && ctx.Err() == nil {
					onError(e)
				}
			}
			continue
		}
		var after string
		for {
			p, err := s.Disk.List(ctx, "", after, 100)
			if err != nil {
				onError(err)
				break
			}
			for _, o := range p.Objects {
				m := &s.locks[slot(o.Key)]
				m.Lock()
				v, e := s.read(ctx, o.Key)
				m.Unlock()
				if e != nil {
					onError(e)
					continue
				}
				if v.Record.Revision == v.SyncedRevision {
					continue
				}
				c, cancel := context.WithTimeout(ctx, 45*time.Second)
				_, e = s.Flush(c, api.Identity{Tenant: v.Record.Tenant, Session: v.Record.Session.ID})
				cancel()
				if e != nil && ctx.Err() == nil {
					onError(fmt.Errorf("publish session metadata: %w", e))
				}
			}
			if p.Next == "" {
				break
			}
			after = p.Next
		}
	}
}
