package credentials_test

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/aexhq/brain/internal/persistence"
	"github.com/aexhq/brain/internal/store"
	"os"
	"path/filepath"
	"testing"
	"time"
)

type keys struct {
	active string
	values map[string][]byte
}

func (k *keys) Active(ctx context.Context) (string, []byte, error) {
	v, e := k.Get(ctx, k.active)
	return k.active, v, e
}
func (k *keys) Get(_ context.Context, id string) ([]byte, error) {
	v, ok := k.values[id]
	if !ok {
		return nil, errors.New("missing key")
	}
	return v, nil
}

func TestCredentialRestartArchiveRotationRevocationAndTampering(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	k := &keys{active: "first", values: map[string][]byte{"first": bytes.Repeat([]byte{1}, 32), "second": bytes.Repeat([]byte{2}, 32)}}
	secret := json.RawMessage(`{"token":"private-session-credential-not-for-models"}`)
	s := core.Session{ID: core.ID(), Tenant: "one", Config: core.Config{Bindings: []core.ToolBinding{{Alias: "data", Provider: "data", Operations: []string{"read"}}}}, CreatedAt: time.Now(), Checkpoint: &core.Checkpoint{ID: core.ID()}}
	var err error
	s.CredentialBindings = []string{"data"}
	s.Credentials, err = credentials.Seal(ctx, k, s, []core.CredentialInput{{Binding: "data", Value: secret}})
	if err != nil {
		t.Fatal(err)
	}
	missing := s
	missing.Credentials = nil
	if credentials.Validate(ctx, k, missing) == nil {
		t.Fatal("missing private row accepted")
	}
	if err := credentials.Validate(ctx, k, s); err != nil {
		t.Fatal(err)
	}
	db, err := store.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	if err := db.Create(ctx, s); err != nil {
		t.Fatal(err)
	}
	dir, _ := db.Dir(s.ID)
	if err := os.WriteFile(filepath.Join(dir, "events.jsonl"), nil, 0600); err != nil {
		t.Fatal(err)
	}
	archiveRoot := t.TempDir()
	archive := &persistence.Archive{Objects: persistence.FileObjects{Root: archiveRoot}, MaxSessionBytes: 16 << 20}
	m, err := archive.Publish(ctx, persistence.Snapshot{Session: s, Directory: dir})
	if err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	db, err = store.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	got, err := db.Get(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	value, err := credentials.Open(ctx, k, got, "data")
	if err != nil || !bytes.Equal(value, secret) {
		t.Fatalf("restart: %s %v", value, err)
	}
	public, _ := json.Marshal(got)
	if bytes.Contains(public, secret) || bytes.Contains(public, []byte("ciphertext")) {
		t.Fatal("private record leaked publicly")
	}
	for _, scanRoot := range []string{root, archiveRoot} {
		if err := filepath.WalkDir(scanRoot, func(p string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if d.IsDir() {
				return nil
			}
			data, err := os.ReadFile(p)
			if err == nil && bytes.Contains(data, []byte("private-session-credential-not-for-models")) {
				t.Errorf("plaintext in %s", p)
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	for _, mutate := range []func(*core.Session){func(s *core.Session) { s.Tenant = "two" }, func(s *core.Session) { s.ID = core.ID() }, func(s *core.Session) { s.Credentials[0].Ciphertext[0] ^= 1 }} {
		copy := got
		copy.Credentials = append([]core.CredentialRecord{}, got.Credentials...)
		copy.Credentials[0].Ciphertext = append([]byte{}, got.Credentials[0].Ciphertext...)
		mutate(&copy)
		if _, err := credentials.Open(ctx, k, copy, "data"); err == nil {
			t.Fatal("credential substitution accepted")
		}
	}
	k.active = "second"
	rotated, err := credentials.Seal(ctx, k, got, []core.CredentialInput{{Binding: "data", Value: value}})
	if err != nil {
		t.Fatal(err)
	}
	got.Credentials = rotated
	if err := db.Metadata.(*store.SQLite).RotateCredentials(ctx, got); err != nil {
		t.Fatal(err)
	}
	if err := db.Metadata.(*store.SQLite).RevokeCredential(ctx, s.Tenant, s.ID, "data"); err != nil {
		t.Fatal(err)
	}
	old, stage, err := archive.Restore(ctx, s.Tenant, s.ID, m.ID, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(stage)
	if err := db.Import(ctx, old.Session); err != nil {
		t.Fatal(err)
	}
	got, err = db.Get(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := credentials.Open(ctx, k, got, "data"); core.ErrorCode(err) != core.Forbidden {
		t.Fatalf("old backup undid revocation: %v", err)
	}
	fresh, err := store.Open(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer fresh.Close()
	if err := fresh.Import(ctx, old.Session); err != nil {
		t.Fatal(err)
	}
	got, err = fresh.Get(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if value, err := credentials.Open(ctx, k, got, "data"); err != nil || !bytes.Equal(value, secret) {
		t.Fatalf("cold restore: %s %v", value, err)
	}
	delete(k.values, "first")
	if _, err := credentials.Open(ctx, k, got, "data"); err == nil {
		t.Fatal("missing key silently accepted")
	}
}
