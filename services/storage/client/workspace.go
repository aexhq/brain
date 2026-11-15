// Package storage provides Storage-owned clients for session coordination.
package storage

import (
	"context"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	api "github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/archive"
	"github.com/aexhq/brain/services/storage/local"
	"hash/fnv"
	"io"
	"sync"
	"time"
)

type Workspace struct {
	MediaOrigins []string
	Client       *api.Client
	Owner        string
	Epoch        uint64
	MaxBytes     int64
	locks        [64]sync.Mutex
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

func (w *Workspace) Close() error                                    { return nil }
func (w *Workspace) ReleaseWorkingCopy(core.Session) error           { return nil }
func (w *Workspace) RemoveWorkingCopy(context.Context, string) error { return nil }
func (w *Workspace) ReadEvents(ctx context.Context, id, after string, limit int, tail bool) ([]core.Event, string, error) {
	r, e := w.Client.Do(ctx, api.Request{Operation: "transcript.read", Identity: w.identity(ctx, id), After: after, Limit: limit, Tail: tail})
	if r.Events == nil {
		r.Events = []core.Event{}
	}
	return r.Events, r.Next, e
}
func (w *Workspace) SyncStatus(ctx context.Context, id string) (core.SyncStatus, error) {
	r, e := w.Client.Do(ctx, api.Request{Operation: "session.sync", Identity: w.identity(ctx, id)})
	if e != nil {
		return core.SyncStatus{}, e
	}
	if r.Status == nil {
		return core.SyncStatus{}, errors.New("storage omitted sync status")
	}
	return *r.Status, nil
}
func (w *Workspace) Get(ctx context.Context, id string) (core.Session, error) {
	r, e := w.record(ctx, w.identity(ctx, id))
	if e == nil && r.Deleted {
		e = core.E(core.NotFound, "session was deleted")
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
	return w.put(ctx, i, api.Record{Session: s, Tenant: s.Tenant, Credentials: s.Credentials}, 0)
}
func (w *Workspace) Save(ctx context.Context, s core.Session) error {
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	_, e := w.Client.Do(ctx, api.Request{Operation: "session.save", Identity: i, Record: &api.Record{Session: s, Tenant: s.Tenant, Credentials: s.Credentials}})
	return e
}
func (w *Workspace) Import(context.Context, core.Session) error {
	return errors.New("restore requires a snapshot reference")
}
func (w *Workspace) InstallRestore(ctx context.Context, s core.Session, reference string) error {
	_, e := w.Client.Do(ctx, api.Request{Operation: "snapshot.restore", Identity: w.identity(ctx, s.ID), Reference: reference, Record: &api.Record{Session: s}})
	return e
}
func (w *Workspace) DiscardRestore(string) error { return nil }
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
	r.Credentials = nil
	r.Transcript = ""
	r.Snapshot = ""
	r.Attachments = ""
	r.Native = ""
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
	r, e := w.record(ctx, i)
	if e != nil {
		return nil, e
	}
	if r.Owner != w.Owner || r.Epoch != w.Epoch {
		return nil, core.E(core.Conflict, "session belongs to another owner")
	}
	read := func(after string, limit int, until int64) ([]core.Event, error) {
		c, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		out, e := w.Client.Do(c, api.Request{Operation: "transcript.read", Identity: i, After: after, Limit: limit, Until: &until})
		return out.Events, e
	}
	appendEvent := func(offset int64, event core.Event) error {
		m := w.lock(i)
		m.Lock()
		defer m.Unlock()
		c, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		_, e := w.Client.Do(c, api.Request{Operation: "transcript.append", Identity: i, Offset: offset, Event: &event, MaxBytes: maxBytes})
		return e
	}
	return store.RemoteLog(r.TranscriptBytes, maxBytes, read, appendEvent), nil
}
func (w *Workspace) PrepareNative(context.Context, core.Session, bool) (string, error) {
	return "", nil
}
func (w *Workspace) ValidateNative(context.Context, core.Session, []core.NativeFile) error {
	return nil
}
func (w *Workspace) ReadNative(ctx context.Context, id string, out io.Writer) error {
	i := w.identity(ctx, id)
	r, e := w.Client.Do(ctx, api.Request{Operation: "state.read", Identity: i})
	if e != nil {
		return e
	}
	if r.Reference == "" {
		return nil
	}
	_, e = w.Client.Download(ctx, i, r.Reference, out, w.MaxBytes)
	return e
}
func (w *Workspace) CaptureNative(ctx context.Context, id string, files []core.NativeFile, input io.Reader) error {
	i := w.identity(ctx, id)
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	ref, _, e := w.Client.Upload(ctx, i, input, w.MaxBytes)
	if e != nil {
		return e
	}
	_, e = w.Client.Do(ctx, api.Request{Operation: "state.capture", Identity: i, Reference: ref, Files: files, MaxBytes: w.MaxBytes})
	return e
}

type Archive struct{ Workspace *Workspace }

func (a *Archive) List(context.Context, string, string, int) ([]core.Session, error) {
	return []core.Session{}, nil
}
func (a *Archive) Delete(ctx context.Context, tenant, id string) error {
	_, e := a.Workspace.Client.Do(ctx, api.Request{Operation: "session.delete", Identity: api.Identity{Tenant: tenant, Session: id}})
	return e
}
func manifest(r api.Record) persistence.Manifest {
	r.Session.Tenant = r.Tenant
	r.Session.Credentials = r.Credentials
	return persistence.Manifest{ID: r.Snapshot, Tenant: r.Tenant, Session: r.Session, Credentials: r.Credentials, HistoryBytes: r.TranscriptBytes, Time: time.Now().UTC()}
}
func (a *Archive) Publish(ctx context.Context, input persistence.Snapshot) (persistence.Manifest, error) {
	w := a.Workspace
	i := api.Identity{Tenant: input.Session.Tenant, Session: input.Session.ID, Owner: w.Owner, Epoch: w.Epoch}
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	out, e := w.Client.Do(ctx, api.Request{Operation: "snapshot.publish", Identity: i})
	if e != nil {
		return persistence.Manifest{}, e
	}
	if out.Record == nil {
		return persistence.Manifest{}, errors.New("storage omitted snapshot")
	}
	return manifest(*out.Record), nil
}
func (a *Archive) Restore(ctx context.Context, tenant, id, reference, parent string) (persistence.Manifest, string, error) {
	w := a.Workspace
	i := api.Identity{Tenant: tenant, Session: id, Owner: w.Owner, Epoch: w.Epoch}
	out, e := w.Client.Do(ctx, api.Request{Operation: "snapshot.read", Identity: i, Reference: reference})
	if e != nil {
		return persistence.Manifest{}, "", e
	}
	if out.Record == nil {
		return persistence.Manifest{}, "", errors.New("storage omitted snapshot")
	}
	return manifest(*out.Record), reference, nil
}

type Snapshot = persistence.Snapshot

func (w *Workspace) Snapshot(ctx context.Context, s core.Session, size int64) (api.Snapshot, error) {
	return api.Snapshot{Session: s, HistoryBytes: size}, nil
}
func (w *Workspace) StageRestore(ctx context.Context, a api.Checkpoints, id, ref string) (api.Manifest, string, error) {
	return a.Restore(ctx, core.Tenant(ctx), id, ref, "")
}
func (w *Workspace) SyncState(context.Context, core.Session) (*api.Manifest, int64, error) {
	return nil, 0, errors.New("use Storage sync status")
}
