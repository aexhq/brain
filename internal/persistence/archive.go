package persistence

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/store"
)

// Complete blocks are uploaded once. Only the bounded partial block repeats.
const BlockBytes int64 = 64 << 10

type Artifact struct {
	Path       string `json:"path"`
	Generation string `json:"generation"`
	Size       int64  `json:"size"`
	Tail       string `json:"tail,omitempty"`
}

type Manifest struct {
	Credentials  []core.CredentialRecord `json:"credentials,omitempty"`
	ID           string                  `json:"id"`
	Tenant       string                  `json:"tenant"`
	Session      core.Session            `json:"session"`
	HistoryBytes int64                   `json:"history_bytes"`
	Files        []Artifact              `json:"files"`
	Time         time.Time               `json:"time"`
}

type Snapshot struct {
	Session      core.Session
	Directory    string
	HistoryBytes int64
}

type Archive struct {
	Objects         Objects
	Metadata        Metadata
	MaxSessionBytes int64
}

func manifestKey(id, reference string) string { return "sessions/" + id + "/checkpoints/" + reference }
func blockKey(id, generation string, index int64) string {
	return "sessions/" + id + "/blocks/" + generation + "/" + strconv.FormatInt(index, 10)
}
func statePath(dir string) string { return filepath.Join(dir, "uploaded.json") }

func strictJSON(data []byte, v any) error {
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(v); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing stored JSON")
	}
	return nil
}

func ReadState(dir string) (*Manifest, error) {
	b, err := os.ReadFile(statePath(dir))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var m Manifest
	if err := strictJSON(b, &m); err != nil {
		return nil, err
	}
	if !core.ValidID(m.ID) || !core.ValidID(m.Session.ID) || m.Session.Checkpoint == nil || !core.ValidID(m.Session.Checkpoint.ID) || m.HistoryBytes < 0 {
		return nil, errors.New("invalid local upload state")
	}
	return &m, nil
}

func saveState(dir string, m Manifest) error {
	b, err := json.Marshal(m)
	if err != nil {
		return err
	}
	return (FileObjects{Root: dir}).Put(context.Background(), "uploaded.json", b, false)
}

// Publish is serialized by the session owner. Snapshot files must retain their
// captured prefixes until this call returns; appending is safe during upload.
func (a *Archive) Publish(ctx context.Context, s Snapshot) (Manifest, error) {
	var zero Manifest
	if !core.ValidID(s.Session.ID) || s.Session.Tenant == "" || s.Session.Checkpoint == nil || s.HistoryBytes < s.Session.Checkpoint.Offset {
		return zero, errors.New("invalid checkpoint snapshot")
	}
	previous, err := ReadState(s.Directory)
	if err != nil {
		return zero, err
	}
	if previous != nil && previous.Session.ID != s.Session.ID {
		return zero, errors.New("upload state does not belong to session")
	}
	if previous != nil {
		if err := a.validate(*previous, s.Session.ID, previous.ID); err != nil {
			return zero, err
		}
	}
	files := []core.NativeFile{{Path: "events.jsonl", Size: s.HistoryBytes, AppendOnly: true}}
	for _, f := range s.Session.Checkpoint.Files {
		if !f.AppendOnly {
			return zero, core.E(core.Unsupported, "this adapter requires an immutable native snapshot before upload")
		}
		f.Path = "native/" + f.Path
		files = append(files, f)
	}
	m := Manifest{Credentials: s.Session.Credentials, ID: core.ID(), Tenant: s.Session.Tenant, Session: s.Session, HistoryBytes: s.HistoryBytes, Time: time.Now().UTC()}
	if len(files) > 128 {
		return zero, errors.New("too many native files")
	}
	var total int64
	for _, file := range files {
		if !validKey(file.Path) || file.Size < 0 || file.Size > a.MaxSessionBytes-total {
			return zero, errors.New("invalid artifact path or checkpoint size")
		}
		total += file.Size
		artifact := Artifact{Path: file.Path, Generation: core.ID(), Size: file.Size}
		var uploaded int64
		if previous != nil {
			for _, p := range previous.Files {
				if p.Path == file.Path && p.Size <= file.Size {
					artifact.Generation = p.Generation
					uploaded = p.Size / BlockBytes
					break
				}
			}
		}
		if err := a.uploadFile(ctx, s.Directory, s.Session.ID, m.ID, file, &artifact, uploaded); err != nil {
			return zero, err
		}
		m.Files = append(m.Files, artifact)
	}
	b, err := json.Marshal(m)
	if err != nil {
		return zero, err
	}
	if err := a.Objects.Put(ctx, manifestKey(s.Session.ID, m.ID), b, true); err != nil {
		return zero, err
	}
	// A failed upload can leave unreachable chunks, never a partially readable checkpoint.
	if err := a.Objects.Put(ctx, "sessions/"+s.Session.ID+"/latest", []byte(m.ID), false); err != nil {
		return zero, err
	}
	if a.Metadata != nil {
		if err := a.Metadata.Put(ctx, s.Session, m.ID); err != nil {
			return zero, err
		}
	}
	if err := saveState(s.Directory, m); err != nil {
		return zero, err
	}
	return m, nil
}

func (a *Archive) uploadFile(ctx context.Context, dir, id, reference string, file core.NativeFile, artifact *Artifact, uploaded int64) error {
	if file.Size == 0 {
		return nil
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return err
	}
	defer root.Close()
	f, err := root.Open(filepath.FromSlash(file.Path))
	if err != nil {
		return err
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Size() < file.Size {
		return errors.New("artifact is shorter than saved boundary")
	}
	for index := uploaded; index < file.Size/BlockBytes; index++ {
		buf := make([]byte, BlockBytes)
		if _, err := f.ReadAt(buf, index*BlockBytes); err != nil {
			return err
		}
		if err := a.Objects.Put(ctx, blockKey(id, artifact.Generation, index), buf, true); err != nil {
			return err
		}
	}
	if remainder := file.Size % BlockBytes; remainder != 0 {
		buf := make([]byte, remainder)
		if _, err := f.ReadAt(buf, file.Size-remainder); err != nil {
			return err
		}
		artifact.Tail = "sessions/" + id + "/tails/" + reference + "/" + artifact.Generation
		if err := a.Objects.Put(ctx, artifact.Tail, buf, true); err != nil {
			return err
		}
	}
	return nil
}

// Restore writes into a new staging directory. It never opens a harness.
func (a *Archive) Restore(ctx context.Context, tenant, id, reference, parent string) (Manifest, string, error) {
	var m Manifest
	if !core.ValidID(id) || (reference != "" && !core.ValidID(reference)) {
		return m, "", core.E(core.Invalid, "invalid checkpoint reference")
	}
	if reference == "" {
		b, err := a.Objects.Get(ctx, "sessions/"+id+"/latest")
		if err != nil {
			return m, "", err
		}
		reference = string(b)
		if !core.ValidID(reference) {
			return m, "", errors.New("invalid latest checkpoint")
		}
	}
	b, err := a.Objects.Get(ctx, manifestKey(id, reference))
	if err != nil {
		return m, "", err
	}
	if err := strictJSON(b, &m); err != nil {
		return m, "", err
	}
	if m.Tenant != tenant {
		return Manifest{}, "", core.E(core.NotFound, "session not found")
	}
	m.Session.Tenant = tenant
	m.Session.Credentials = m.Credentials
	if err := a.validate(m, id, reference); err != nil {
		return m, "", err
	}
	dir, err := os.MkdirTemp(parent, ".restore-")
	if err != nil {
		return m, "", err
	}
	ok := false
	defer func() {
		if !ok {
			_ = os.RemoveAll(dir)
		}
	}()
	if err := os.MkdirAll(filepath.Join(dir, "native"), 0700); err != nil {
		return m, "", err
	}
	for _, file := range m.Files {
		p := filepath.Join(dir, filepath.FromSlash(file.Path))
		if err := os.MkdirAll(filepath.Dir(p), 0700); err != nil {
			return m, "", err
		}
		f, err := os.OpenFile(p, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return m, "", err
		}
		err = a.downloadFile(ctx, m.Session.ID, file, f)
		err = errors.Join(err, f.Close())
		if err != nil {
			return m, "", err
		}
	}
	log, err := store.OpenLog(filepath.Join(dir, "events.jsonl"), a.MaxSessionBytes)
	if err != nil {
		return m, "", err
	}
	size := log.Size()
	err = log.Close()
	if err != nil {
		return m, "", err
	}
	if size != m.HistoryBytes {
		return m, "", errors.New("checkpoint ends inside an event")
	}
	if err := saveState(dir, m); err != nil {
		return m, "", err
	}
	ok = true
	return m, dir, nil
}

func (a *Archive) validate(m Manifest, id, reference string) error {
	cp := m.Session.Checkpoint
	if m.ID != reference || m.Session.ID != id || cp == nil || !core.ValidID(cp.ID) || cp.Offset < 0 || cp.Offset > m.HistoryBytes || len(m.Files) > 128 {
		return errors.New("inconsistent checkpoint manifest")
	}
	expected := map[string]int64{"events.jsonl": m.HistoryBytes}
	for _, f := range cp.Files {
		if !f.AppendOnly || !validKey(f.Path) {
			return errors.New("invalid native checkpoint")
		}
		key := "native/" + f.Path
		if _, ok := expected[key]; ok {
			return errors.New("duplicate checkpoint file")
		}
		expected[key] = f.Size
	}
	var total int64
	for _, f := range m.Files {
		size, ok := expected[f.Path]
		if !ok || !validKey(f.Path) || size != f.Size || f.Size < 0 || f.Size > a.MaxSessionBytes-total || !core.ValidID(f.Generation) {
			return errors.New("invalid checkpoint artifact")
		}
		delete(expected, f.Path)
		total += f.Size
		if f.Size%BlockBytes == 0 {
			if f.Tail != "" {
				return errors.New("unexpected artifact tail")
			}
		} else if !validKey(f.Tail) || !strings.HasPrefix(f.Tail, "sessions/"+id+"/tails/"+reference+"/") {
			return errors.New("invalid artifact tail")
		}
	}
	if len(expected) != 0 {
		return errors.New("checkpoint is missing artifacts")
	}
	return nil
}

func (a *Archive) downloadFile(ctx context.Context, id string, f Artifact, w io.Writer) error {
	for index := int64(0); index < f.Size/BlockBytes; index++ {
		b, err := a.Objects.Get(ctx, blockKey(id, f.Generation, index))
		if err != nil {
			return err
		}
		if int64(len(b)) != BlockBytes {
			return fmt.Errorf("invalid block length for %s", f.Path)
		}
		if _, err := w.Write(b); err != nil {
			return err
		}
	}
	if remainder := f.Size % BlockBytes; remainder != 0 {
		b, err := a.Objects.Get(ctx, f.Tail)
		if err != nil {
			return err
		}
		if int64(len(b)) != remainder {
			return errors.New("invalid tail length")
		}
		if _, err := w.Write(b); err != nil {
			return err
		}
	}
	return nil
}

// List returns the optional eventual discovery index. An archive without an
// index contributes no remote rows; local discovery remains authoritative.
func (a *Archive) List(ctx context.Context, tenant, after string, limit int) ([]core.Session, error) {
	if a.Metadata == nil {
		return []core.Session{}, nil
	}
	return a.Metadata.List(ctx, tenant, after, limit)
}
