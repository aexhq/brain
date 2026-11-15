package server

import (
	"archive/tar"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"path"
	"reflect"
	"strings"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/local"
)

type nativeState struct {
	Content string            `json:"content"`
	Files   []core.NativeFile `json:"files"`
}
type snapshot struct {
	Record api.Record `json:"record"`
}

func (s *Server) owned(ctx context.Context, i api.Identity) (api.Record, error) {
	r, err := s.Store.Get(ctx, i)
	if err != nil {
		return r, err
	}
	if r.Deleted {
		return r, core.E(core.NotFound, "session was deleted")
	}
	if i.Owner == "" || i.Epoch == 0 || r.Owner != i.Owner || r.Epoch != i.Epoch {
		return r, core.E(core.Conflict, "session owner was fenced")
	}
	return r, nil
}

func (s *Server) append(ctx context.Context, in api.Request) (api.Record, error) {
	r, err := s.owned(ctx, in.Identity)
	if err != nil {
		return r, err
	}
	if in.Event == nil || !core.ValidID(in.Event.ID) || in.Event.Type == "" || in.Event.Time.IsZero() || len(in.Event.Data) == 0 {
		return r, core.E(core.Invalid, "event is required")
	}
	if r.TranscriptBytes != in.Offset {
		return r, core.E(core.Conflict, "transcript writer is stale")
	}
	event := *in.Event
	event.Cursor = ""
	data, err := json.Marshal(event)
	if err != nil {
		return r, err
	}
	data = append(data, '\n')
	if len(data) > store.MaxRecord || in.MaxBytes < 1 || r.TranscriptBytes+int64(len(data)) > in.MaxBytes {
		return r, core.E(core.Overloaded, "session log limit reached")
	}
	payload, size, err := api.Upload(ctx, s.Store, in.Identity, bytes.NewReader(data), store.MaxRecord)
	if err != nil {
		return r, err
	}
	node, err := json.Marshal(api.Transcript{Previous: r.Transcript, Payload: payload, Offset: r.TranscriptBytes, Size: size})
	if err != nil {
		return r, err
	}
	refs := []string{payload}
	if r.Transcript != "" {
		refs = append(refs, r.Transcript)
	}
	r.Transcript, err = s.Store.PutObject(ctx, in.Identity, api.Object{Data: node, References: refs})
	if err != nil {
		return r, err
	}
	r.TranscriptBytes += size
	return s.Store.Put(ctx, in.Identity, r, r.Revision)
}

// capture validates the requested file boundary and retains only those files.
// Both archive streams are bounded; staging and tier selection stay in Storage.
func (s *Server) capture(ctx context.Context, in api.Request) (api.Record, error) {
	r, err := s.owned(ctx, in.Identity)
	if err != nil {
		return r, err
	}
	if in.MaxBytes < 1 || len(in.Files) > 4096 {
		return r, core.E(core.Invalid, "invalid state limits")
	}
	wanted := map[string]int64{}
	var total int64
	for _, f := range in.Files {
		if f.Path == "" || strings.Contains(f.Path, "\\") || path.Clean(f.Path) != f.Path || strings.HasPrefix(f.Path, "/") || f.Path == ".." || strings.HasPrefix(f.Path, "../") || f.Size < 0 || f.Size > in.MaxBytes-total {
			return r, core.E(core.Invalid, "invalid state file")
		}
		name := "native/" + f.Path
		if _, exists := wanted[name]; exists {
			return r, core.E(core.Invalid, "duplicate state file")
		}
		wanted[name] = f.Size
		total += f.Size
	}
	input, download := io.Pipe()
	downloaded := make(chan error, 1)
	go func() {
		_, e := api.Download(ctx, s.Store, in.Identity, in.Reference, download, in.MaxBytes)
		_ = download.CloseWithError(e)
		downloaded <- e
	}()
	output, upload := io.Pipe()
	copied := make(chan error, 1)
	go func() {
		tr, tw := tar.NewReader(input), tar.NewWriter(upload)
		e := func() error {
			for {
				h, e := tr.Next()
				if e == io.EOF {
					break
				}
				if e != nil {
					return e
				}
				name := strings.TrimPrefix(h.Name, "./")
				size, ok := wanted[name]
				if !ok {
					continue
				}
				if h.Typeflag != tar.TypeReg || h.Size != size {
					return core.E(core.Invalid, "inconsistent state file")
				}
				if e = tw.WriteHeader(&tar.Header{Name: name, Mode: 0600, Size: size}); e != nil {
					return e
				}
				if _, e = io.CopyN(tw, tr, size); e != nil {
					return e
				}
				delete(wanted, name)
			}
			if len(wanted) != 0 {
				return core.E(core.Invalid, "state archive is incomplete")
			}
			_, e := io.Copy(io.Discard, input)
			return e
		}()
		e = errors.Join(e, tw.Close())
		_ = input.CloseWithError(e)
		_ = upload.CloseWithError(e)
		copied <- e
	}()
	content, _, err := api.Upload(ctx, s.Store, in.Identity, output, in.MaxBytes)
	_ = output.CloseWithError(err)
	err = errors.Join(err, <-copied, <-downloaded)
	if err != nil {
		return r, err
	}
	data, err := json.Marshal(nativeState{Content: content, Files: in.Files})
	if err != nil {
		return r, err
	}
	r.Native, err = s.Store.PutObject(ctx, in.Identity, api.Object{Data: data, References: []string{content}})
	if err != nil {
		return r, err
	}
	return s.Store.Put(ctx, in.Identity, r, r.Revision)
}

func (s *Server) native(ctx context.Context, i api.Identity, r api.Record) (nativeState, error) {
	var state nativeState
	if r.Native == "" {
		return state, nil
	}
	object, err := s.Store.GetObject(ctx, i, r.Native)
	if err != nil {
		return state, err
	}
	err = json.Unmarshal(object.Data, &state)
	return state, err
}

func (s *Server) save(ctx context.Context, in api.Request) (api.Record, error) {
	r, err := s.owned(ctx, in.Identity)
	if err != nil {
		return r, err
	}
	if in.Record == nil || in.Record.Session.ID != in.Identity.Session || in.Record.Tenant != in.Identity.Tenant {
		return r, core.E(core.Invalid, "session is required")
	}
	previous := r.Session.Checkpoint
	r.Session, r.Credentials = in.Record.Session, in.Record.Credentials
	if cp := r.Session.Checkpoint; cp != nil && (previous == nil || cp.ID != previous.ID) {
		state, e := s.native(ctx, in.Identity, r)
		if e != nil {
			return r, e
		}
		if len(cp.Files) != len(state.Files) || len(cp.Files) > 0 && !reflect.DeepEqual(cp.Files, state.Files) {
			return r, core.E(core.Conflict, "checkpoint differs from captured state")
		}
		if cp.Offset < 0 || cp.Offset > r.TranscriptBytes {
			return r, core.E(core.Invalid, "invalid checkpoint boundary")
		}
		r.Snapshot, err = s.snapshot(ctx, in.Identity, r)
		if err != nil {
			return r, err
		}
	}
	return s.Store.Put(ctx, in.Identity, r, r.Revision)
}

func (s *Server) snapshot(ctx context.Context, i api.Identity, r api.Record) (string, error) {
	if r.Session.Checkpoint == nil {
		return "", core.E(core.Invalid, "session has no usable checkpoint")
	}
	r.Snapshot = ""
	data, err := json.Marshal(snapshot{Record: r})
	if err != nil {
		return "", err
	}
	refs := []string{}
	for _, ref := range []string{r.Native, r.Transcript, r.Attachments} {
		if ref != "" {
			refs = append(refs, ref)
		}
	}
	return s.Store.PutObject(ctx, i, api.Object{Data: data, References: refs})
}

func (s *Server) readSnapshot(ctx context.Context, i api.Identity, ref string) (api.Record, error) {
	object, err := s.Store.GetObject(ctx, i, ref)
	if err != nil {
		return api.Record{}, err
	}
	var v snapshot
	d := json.NewDecoder(bytes.NewReader(object.Data))
	d.DisallowUnknownFields()
	if err = d.Decode(&v); err != nil {
		return v.Record, err
	}
	if d.Decode(new(any)) != io.EOF || v.Record.Session.Checkpoint == nil {
		return v.Record, errors.New("invalid snapshot")
	}
	if v.Record.Tenant != i.Tenant || v.Record.Session.ID != i.Session {
		return v.Record, core.E(core.Forbidden, "snapshot belongs to another session")
	}
	v.Record.Snapshot = ref
	return v.Record, nil
}

func (s *Server) boundary(ctx context.Context, i api.Identity, r api.Record, until int64) (api.Record, error) {
	if until < 0 || until > r.TranscriptBytes {
		return r, core.E(core.CursorUnavailable, "invalid transcript boundary")
	}
	for r.TranscriptBytes > until {
		o, err := s.Store.GetObject(ctx, i, r.Transcript)
		if err != nil {
			return r, err
		}
		var n api.Transcript
		if json.Unmarshal(o.Data, &n) != nil || n.Offset < 0 || n.Size <= 0 || n.Offset+n.Size != r.TranscriptBytes {
			return r, errors.New("invalid transcript chain")
		}
		r.Transcript, r.TranscriptBytes = n.Previous, n.Offset
	}
	if r.TranscriptBytes != until {
		return r, core.E(core.CursorUnavailable, "invalid transcript boundary")
	}
	return r, nil
}
