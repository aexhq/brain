// Package storage adapts Storage's domain API to the session coordinator. Files
// here are disposable working copies; Storage owns acknowledged durable state.
package storage

import (
	"archive/tar"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"hash/fnv"
	"io"
	"os"
	"path/filepath"
	"sync"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/brain/internal/archive"
	"github.com/aexhq/brain/services/brain/internal/storage"
	api "github.com/aexhq/brain/services/storage/api"
)

type Workspace struct {
	MediaOrigins []string
	Client       *api.Client
	Owner        string
	Epoch        uint64
	Directory    string
	MaxBytes     int64
	locks        [64]sync.Mutex
}
type transcript = api.Transcript

type snapshot struct {
	Record api.Record `json:"record"`
	Native string     `json:"native"`
}

func (w *Workspace) identity(ctx context.Context, id string) api.Identity {
	return api.Identity{Tenant: core.Tenant(ctx), Session: id, Owner: w.Owner, Epoch: w.Epoch}
}
func (w *Workspace) lock(i api.Identity) *sync.Mutex {
	h := fnv.New32a()
	_, _ = h.Write([]byte(core.SessionKey(i.Tenant, i.Session)))
	return &w.locks[h.Sum32()%64]
}
func (w *Workspace) record(ctx context.Context, i api.Identity) (api.Record, error) {
	r, e := w.Client.Do(ctx, api.Request{Operation: "session.get", Identity: i})
	if e != nil {
		return api.Record{}, e
	}
	if r.Record == nil {
		return api.Record{}, errors.New("storage omitted session")
	}
	r.Record.Session.Tenant = r.Record.Tenant
	r.Record.Session.Credentials = r.Record.Credentials
	return *r.Record, nil
}
func (w *Workspace) put(ctx context.Context, i api.Identity, r api.Record, revision uint64) error {
	_, e := w.Client.Do(ctx, api.Request{Operation: "session.put", Identity: i, Record: &r, ExpectedRevision: revision})
	return e
}
func (w *Workspace) Root() string { return w.Directory }
func (w *Workspace) Dir(key string) (string, error) {
	if _, _, e := core.SplitSessionKey(key); e != nil {
		return "", e
	}
	return filepath.Join(w.Directory, "sessions", core.SessionDirectory(key)), nil
}
func (w *Workspace) Close() error { return nil }
func (w *Workspace) ReleaseWorkingCopy(s core.Session) error {
	dir, err := w.Dir(core.SessionKey(s.Tenant, s.ID))
	if err != nil {
		return err
	}
	return os.RemoveAll(dir)
}
func (w *Workspace) ReadEvents(ctx context.Context, id, after string, limit int, tail bool) ([]core.Event, string, error) {
	r, err := w.Client.Do(ctx, api.Request{Operation: "transcript.read", Identity: w.identity(ctx, id), After: after, Limit: limit, Tail: tail})
	if r.Events == nil {
		r.Events = []core.Event{}
	}
	return r.Events, r.Next, err
}
func (w *Workspace) SyncStatus(ctx context.Context, id string) (core.SyncStatus, error) {
	r, err := w.Client.Do(ctx, api.Request{Operation: "session.sync", Identity: w.identity(ctx, id)})
	if err != nil {
		return core.SyncStatus{}, err
	}
	if r.Status == nil {
		return core.SyncStatus{}, errors.New("storage omitted synchronization status")
	}
	return *r.Status, nil
}
func (w *Workspace) Get(ctx context.Context, id string) (core.Session, error) {
	r, e := w.record(ctx, w.identity(ctx, id))
	if e == nil && r.Deleted {
		e = core.E(core.NotFound, "session not found")
	}
	return r.Session, e
}
func (w *Workspace) Deleted(ctx context.Context, id string) (string, error) {
	r, e := w.record(ctx, w.identity(ctx, id))
	if core.ErrorCode(e) == core.NotFound {
		return "", nil
	}
	if e != nil {
		return "", e
	}
	if r.Deleted {
		return r.Tenant, nil
	}
	return "", nil
}
func (w *Workspace) Create(ctx context.Context, s core.Session) error {
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	r := api.Record{Session: s, Tenant: s.Tenant, Credentials: s.Credentials}
	if e := w.put(ctx, i, r, 0); e != nil {
		return e
	}
	d, e := w.Dir(core.SessionKey(s.Tenant, s.ID))
	if e != nil {
		return e
	}
	return os.MkdirAll(filepath.Join(d, "native"), 0700)
}
func (w *Workspace) Save(ctx context.Context, s core.Session) error {
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	r, e := w.record(ctx, i)
	if e != nil {
		return e
	}
	old := r.Session.Checkpoint
	r.Session = s
	r.Credentials = s.Credentials
	if s.Checkpoint != nil && (old == nil || old.ID != s.Checkpoint.ID) {
		ref, e := w.uploadSnapshot(ctx, i, r)
		if e != nil {
			return e
		}
		r.Snapshot = ref
	}
	return w.put(ctx, i, r, r.Revision)
}
func (w *Workspace) Import(ctx context.Context, s core.Session) error {
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	dir, err := w.Dir(core.SessionKey(s.Tenant, s.ID))
	if err != nil {
		return err
	}
	data, err := os.ReadFile(filepath.Join(dir, "restore-record.json"))
	if err != nil {
		return err
	}
	var restored api.Record
	if err = json.Unmarshal(data, &restored); err != nil {
		return err
	}
	if restored.Tenant != s.Tenant || restored.Session.ID != s.ID {
		return errors.New("restore identity changed")
	}
	current, err := w.record(ctx, i)
	if err != nil {
		return err
	}
	restored.Session = s
	restored.Credentials = current.Credentials
	restored.Session.Credentials = current.Credentials
	if err = w.put(ctx, i, restored, current.Revision); err != nil {
		return err
	}
	return os.Remove(filepath.Join(dir, "restore-record.json"))
}
func (w *Workspace) Delete(ctx context.Context, s core.Session) error {
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	r, e := w.record(ctx, i)
	if e != nil {
		return e
	}
	r.Deleted = true
	r.Session = core.Session{ID: s.ID, Status: "deleted"}
	r.Session.Credentials = nil
	r.Credentials = nil
	r.Transcript = ""
	r.Snapshot = ""
	r.Attachments = ""
	r.TranscriptBytes = 0
	return w.put(ctx, i, r, r.Revision)
}
func (w *Workspace) List(ctx context.Context, tenant, after string, limit int) ([]core.Session, error) {
	cursor := after
	out := []core.Session{}
	for {
		p, e := w.Client.Do(ctx, api.Request{Operation: "session.list", Identity: api.Identity{Tenant: tenant}, After: cursor, Limit: min(100, limit-len(out))})
		if e != nil {
			return nil, e
		}
		for _, r := range p.Records {
			if !r.Deleted && r.Session.ID > after {
				r.Session.Tenant = tenant
				out = append(out, r.Session)
			}
		}
		if p.Next == "" || len(out) == limit {
			break
		}
		cursor = p.Next
	}
	return out, nil
}
func (w *Workspace) Scan(context.Context, string, int) ([]core.Session, error) {
	return nil, errors.New("storage owns background replication")
}

func (w *Workspace) OpenLog(ctx context.Context, s core.Session, maxBytes int64) (*store.Log, error) {
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	r, e := w.record(ctx, i)
	if e != nil {
		return nil, e
	}
	if r.Owner != w.Owner || r.Epoch != w.Epoch {
		return nil, core.E(core.Conflict, "session belongs to another owner")
	}
	d, e := w.Dir(core.SessionKey(s.Tenant, s.ID))
	if e != nil {
		return nil, e
	}
	if e = os.MkdirAll(filepath.Join(d, "native"), 0700); e != nil {
		return nil, e
	}
	if e = w.downloadTranscript(ctx, i, r, d); e != nil {
		return nil, e
	}
	if r.Snapshot != "" {
		v, e := w.readSnapshot(ctx, i, r.Snapshot)
		if e != nil {
			return nil, e
		}
		if e = w.downloadNative(ctx, i, v.Native, filepath.Join(d, "native")); e != nil {
			return nil, e
		}
	}
	l, e := store.OpenLog(filepath.Join(d, "events.jsonl"), maxBytes)
	if e != nil {
		return nil, e
	}
	l.BeforeAppend(func(offset int64, event core.Event) error {
		c, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		return w.append(c, i, offset, event)
	})
	return l, nil
}
func (w *Workspace) append(ctx context.Context, i api.Identity, offset int64, event core.Event) error {
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	r, e := w.record(ctx, i)
	if e != nil {
		return e
	}
	if r.TranscriptBytes != offset {
		return core.E(core.Conflict, "transcript writer is stale")
	}
	event.Cursor = ""
	b, e := json.Marshal(event)
	if e != nil {
		return e
	}
	b = append(b, '\n')
	payload, size, e := w.Client.Upload(ctx, i, bytes.NewReader(b), store.MaxRecord)
	if e != nil {
		return e
	}
	n := transcript{Previous: r.Transcript, Payload: payload, Offset: offset, Size: size}
	data, _ := json.Marshal(n)
	refs := []string{payload}
	if n.Previous != "" {
		refs = append(refs, n.Previous)
	}
	ref, e := w.Client.PutObject(ctx, i, api.Object{Data: data, References: refs})
	if e != nil {
		return e
	}
	r.Transcript = ref
	r.TranscriptBytes += size
	return w.put(ctx, i, r, r.Revision)
}
func (w *Workspace) downloadTranscript(ctx context.Context, i api.Identity, r api.Record, dir string) error {
	if r.TranscriptBytes > w.MaxBytes {
		return errors.New("retained transcript exceeds working-set limit")
	}
	f, e := os.OpenFile(filepath.Join(dir, "events.jsonl"), os.O_CREATE|os.O_TRUNC|os.O_RDWR, 0600)
	if e != nil {
		return e
	}
	defer f.Close()
	ref := r.Transcript
	end := r.TranscriptBytes
	for ref != "" {
		o, e := w.Client.GetObject(ctx, i, ref)
		if e != nil {
			return e
		}
		var n transcript
		if e = json.Unmarshal(o.Data, &n); e != nil {
			return e
		}
		if n.Offset < 0 || n.Size <= 0 || n.Size > store.MaxRecord || n.Offset+n.Size != end {
			return errors.New("inconsistent transcript chain")
		}
		if _, e = f.Seek(n.Offset, io.SeekStart); e != nil {
			return e
		}
		size, e := w.Client.Download(ctx, i, n.Payload, f, n.Size)
		if e != nil {
			return e
		}
		if size != n.Size {
			return errors.New("incomplete transcript block")
		}
		end = n.Offset
		ref = n.Previous
	}
	if end != 0 {
		return errors.New("transcript chain is incomplete")
	}
	return f.Sync()
}
func (w *Workspace) uploadSnapshot(ctx context.Context, i api.Identity, r api.Record) (string, error) {
	d, e := w.Dir(core.SessionKey(i.Tenant, i.Session))
	if e != nil {
		return "", e
	}
	root, e := os.OpenRoot(filepath.Join(d, "native"))
	if e != nil {
		return "", e
	}
	defer root.Close()
	reader, writer := io.Pipe()
	done := make(chan error, 1)
	go func() {
		tw := tar.NewWriter(writer)
		var err error
		for _, v := range r.Session.Checkpoint.Files {
			if !filepath.IsLocal(v.Path) {
				err = errors.New("invalid snapshot path")
				break
			}
			f, e := root.Open(v.Path)
			if e != nil {
				err = e
				break
			}
			info, e := f.Stat()
			if e == nil && (!info.Mode().IsRegular() || info.Size() < v.Size) {
				e = errors.New("invalid native snapshot file")
			}
			if e == nil {
				e = tw.WriteHeader(&tar.Header{Name: v.Path, Mode: 0600, Size: v.Size})
			}
			if e == nil {
				_, e = io.CopyN(tw, f, v.Size)
			}
			f.Close()
			if e != nil {
				err = e
				break
			}
		}
		err = errors.Join(err, tw.Close())
		_ = writer.CloseWithError(err)
		done <- err
	}()
	native, _, e := w.Client.Upload(ctx, i, reader, w.MaxBytes)
	_ = reader.CloseWithError(e)
	e = errors.Join(e, <-done)
	if e != nil {
		return "", e
	}
	copy := r
	copy.Snapshot = ""
	b, e := json.Marshal(snapshot{Record: copy, Native: native})
	if e != nil {
		return "", e
	}
	refs := []string{native}
	if r.Transcript != "" {
		refs = append(refs, r.Transcript)
	}
	if r.Attachments != "" {
		refs = append(refs, r.Attachments)
	}
	return w.Client.PutObject(ctx, i, api.Object{Data: b, References: refs})
}
func (w *Workspace) readSnapshot(ctx context.Context, i api.Identity, ref string) (snapshot, error) {
	var v snapshot
	o, e := w.Client.GetObject(ctx, i, ref)
	if e != nil {
		return v, e
	}
	e = json.Unmarshal(o.Data, &v)
	if e == nil && (v.Record.Tenant != i.Tenant || v.Record.Session.ID != i.Session) {
		e = core.E(core.Forbidden, "snapshot belongs to another session")
	}
	v.Record.Session.Tenant = i.Tenant
	v.Record.Session.Credentials = v.Record.Credentials
	return v, e
}
func (w *Workspace) downloadNative(ctx context.Context, i api.Identity, ref, dir string) error {
	f, e := os.CreateTemp(w.Directory, "native-*.tar")
	if e != nil {
		return e
	}
	defer os.Remove(f.Name())
	defer f.Close()
	if _, e = w.Client.Download(ctx, i, ref, f, w.MaxBytes); e != nil {
		return e
	}
	if _, e = f.Seek(0, io.SeekStart); e != nil {
		return e
	}
	if e = os.MkdirAll(dir, 0700); e != nil {
		return e
	}
	root, e := os.OpenRoot(dir)
	if e != nil {
		return e
	}
	defer root.Close()
	tr := tar.NewReader(f)
	for {
		h, e := tr.Next()
		if e == io.EOF {
			return nil
		}
		if e != nil {
			return e
		}
		if h.Typeflag != tar.TypeReg || !filepath.IsLocal(h.Name) {
			return errors.New("invalid native archive entry")
		}
		if e = root.MkdirAll(filepath.Dir(h.Name), 0700); e != nil {
			return e
		}
		out, e := root.OpenFile(h.Name, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0600)
		if e != nil {
			return e
		}
		_, e = io.Copy(out, tr)
		if e == nil {
			e = out.Sync()
		}
		e = errors.Join(e, out.Close())
		if e != nil {
			return e
		}
	}
}

// Archive translates existing coordinator snapshot operations into Storage RPC.
// It contains no external-provider implementation or shared filesystem mount.
type Archive struct{ Workspace *Workspace }

func (a *Archive) List(context.Context, string, string, int) ([]core.Session, error) {
	return []core.Session{}, nil
}
func (a *Archive) Delete(ctx context.Context, tenant, id string) error {
	_, e := a.Workspace.Client.Do(ctx, api.Request{Operation: "session.delete", Identity: api.Identity{Tenant: tenant, Session: id}})
	return e
}
func (a *Archive) Publish(ctx context.Context, input persistence.Snapshot) (persistence.Manifest, error) {
	w := a.Workspace
	i := api.Identity{Tenant: input.Session.Tenant, Session: input.Session.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	r, e := w.record(ctx, i)
	if e != nil {
		return persistence.Manifest{}, e
	}
	ref, e := w.uploadSnapshot(ctx, i, r)
	if e != nil {
		return persistence.Manifest{}, e
	}
	r.Snapshot = ref
	if e = w.put(ctx, i, r, r.Revision); e != nil {
		return persistence.Manifest{}, e
	}
	if _, e = w.Client.Do(ctx, api.Request{Operation: "session.flush", Identity: i}); e != nil {
		return persistence.Manifest{}, e
	}
	manifest := persistence.Manifest{ID: ref, Tenant: i.Tenant, Session: r.Session, Credentials: r.Credentials, HistoryBytes: r.TranscriptBytes, Time: time.Now().UTC()}
	b, e := json.Marshal(manifest)
	if e != nil {
		return manifest, e
	}
	e = os.WriteFile(filepath.Join(input.Directory, "uploaded.json"), b, 0600)
	return manifest, e
}
func (a *Archive) Restore(ctx context.Context, tenant, id, reference, parent string) (persistence.Manifest, string, error) {
	w := a.Workspace
	i := api.Identity{Tenant: tenant, Session: id, Owner: w.Owner, Epoch: w.Epoch}
	var empty persistence.Manifest
	current, e := w.record(ctx, i)
	if e != nil {
		return empty, "", e
	}
	if current.Deleted {
		return empty, "", core.E(core.NotFound, "session was deleted")
	}
	if reference == "" {
		reference = current.Snapshot
	}
	if reference == "" {
		return empty, "", core.E(core.NotFound, "session has no native snapshot")
	}
	v, e := w.readSnapshot(ctx, i, reference)
	if e != nil {
		return empty, "", e
	}
	v.Record.Credentials = current.Credentials
	v.Record.Session.Credentials = current.Credentials
	d, e := os.MkdirTemp(parent, ".restore-")
	if e != nil {
		return empty, "", e
	}
	ok := false
	defer func() {
		if !ok {
			_ = os.RemoveAll(d)
		}
	}()
	if e = w.downloadNative(ctx, i, v.Native, filepath.Join(d, "native")); e != nil {
		return empty, "", e
	}
	if e = w.downloadTranscript(ctx, i, v.Record, d); e != nil {
		return empty, "", e
	}
	manifest := persistence.Manifest{ID: reference, Tenant: tenant, Session: v.Record.Session, Credentials: current.Credentials, HistoryBytes: v.Record.TranscriptBytes, Time: time.Now().UTC()}
	b, _ := json.Marshal(manifest)
	if e = os.WriteFile(filepath.Join(d, "uploaded.json"), b, 0600); e != nil {
		return empty, "", e
	}
	// Import runs only after the coordinator has reserved the entry and installed
	// this staged directory. Preserve the intended transcript head until then.
	v.Record.Snapshot = reference
	b, _ = json.Marshal(v.Record)
	if e = os.WriteFile(filepath.Join(d, "restore-record.json"), b, 0600); e != nil {
		return empty, "", e
	}
	ok = true
	return manifest, d, nil
}

// Snapshot is the coherent native-state and transcript boundary published by Archive.
type Snapshot = persistence.Snapshot
