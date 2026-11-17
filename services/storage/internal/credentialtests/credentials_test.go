package credentials_test

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	persistence "github.com/aexhq/brain/services/storage/archive"
	"github.com/aexhq/brain/services/storage/internal/credentialcipher"
	store "github.com/aexhq/brain/services/storage/local"
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

func TestStorageCredentialLifecycle(t *testing.T) {
	ctx := core.WithTenant(context.Background(), "one")
	k := &keys{active: "first", values: map[string][]byte{"first": bytes.Repeat([]byte{1}, 32), "second": bytes.Repeat([]byte{2}, 32)}}
	secret := json.RawMessage(`{"token":"private-session-credential-not-for-models"}`)
	s := core.Session{ID: core.ID(), Tenant: "one", CreatedAt: time.Now(), Checkpoint: &core.Checkpoint{ID: core.ID()}, CredentialBindings: []string{"data"}, Credentials: []credential.Record{{Binding: "data", Value: secret}}}
	root, remote := t.TempDir(), t.TempDir()
	db, err := store.Open(root, k)
	if err != nil {
		t.Fatal(err)
	}
	if err = db.Create(ctx, s); err != nil {
		t.Fatal(err)
	}
	dir, _ := db.Dir(core.SessionKey(s.Tenant, s.ID))
	if err = os.WriteFile(filepath.Join(dir, "events.jsonl"), nil, 0600); err != nil {
		t.Fatal(err)
	}
	archive := &persistence.Archive{Keys: k, Objects: persistence.FileObjects{Root: remote}, MaxSessionBytes: 16 << 20}
	m, err := archive.Publish(ctx, persistence.Snapshot{Session: s, Directory: dir})
	if err != nil {
		t.Fatal(err)
	}
	if err = db.Close(); err != nil {
		t.Fatal(err)
	}
	db, err = store.Open(root, k)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	got, err := db.Get(ctx, s.ID)
	if err != nil || !bytes.Equal(got.Credentials[0].Value, secret) {
		t.Fatalf("restart: %v", err)
	}
	public, _ := json.Marshal(got)
	if bytes.Contains(public, secret) || bytes.Contains(public, []byte("ciphertext")) {
		t.Fatal("private state leaked publicly")
	}
	for _, root := range []string{root, remote} {
		err = filepath.WalkDir(root, func(p string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if d.IsDir() {
				return nil
			}
			b, e := os.ReadFile(p)
			if bytes.Contains(b, []byte("private-session-credential-not-for-models")) {
				t.Errorf("plaintext in %s", p)
			}
			return e
		})
		if err != nil {
			t.Fatal(err)
		}
	}
	// A copied or corrupted persisted envelope must fail at the storage read.
	raw, err := sql.Open("sqlite", filepath.Join(root, "metadata.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer raw.Close()
	var original []byte
	if err = raw.QueryRow(`SELECT data FROM credentials`).Scan(&original); err != nil {
		t.Fatal(err)
	}
	for _, mutate := range []func(*credentialcipher.Record){
		func(r *credentialcipher.Record) { r.Ciphertext[0] ^= 1 },
		func(r *credentialcipher.Record) { r.Binding = "other" },
		func(r *credentialcipher.Record) { r.Key = "missing" },
	} {
		var r credentialcipher.Record
		if err = json.Unmarshal(original, &r); err != nil {
			t.Fatal(err)
		}
		mutate(&r)
		data, _ := json.Marshal(r)
		if _, err = raw.Exec(`UPDATE credentials SET data=?`, data); err != nil {
			t.Fatal(err)
		}
		if _, err = db.Get(ctx, s.ID); err == nil {
			t.Fatal("tampered stored credential accepted")
		}
	}
	if _, err = raw.Exec(`UPDATE credentials SET data=?`, original); err != nil {
		t.Fatal(err)
	}
	k.active = "second"
	if err = db.Metadata.(*store.SQLite).RotateCredentials(ctx, got); err != nil {
		t.Fatal(err)
	}
	delete(k.values, "first")
	if _, err = db.Get(ctx, s.ID); err != nil {
		t.Fatalf("rotation still needs old key: %v", err)
	}
	if _, _, err = archive.Restore(ctx, s.Tenant, s.ID, m.ID, t.TempDir()); err == nil {
		t.Fatal("archive accepted missing key")
	}
	k.values["first"] = bytes.Repeat([]byte{1}, 32)
	if err = db.Metadata.(*store.SQLite).RevokeCredential(ctx, s.Tenant, s.ID, "data"); err != nil {
		t.Fatal(err)
	}
	old, stage, err := archive.Restore(ctx, s.Tenant, s.ID, m.ID, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(stage)
	if err = db.Import(ctx, old.Session); err != nil {
		t.Fatal(err)
	}
	got, err = db.Get(ctx, s.ID)
	if err != nil || !got.Credentials[0].Revoked || len(got.Credentials[0].Value) != 0 {
		t.Fatalf("restore undid revocation: %v", err)
	}
	fresh, err := store.Open(t.TempDir(), k)
	if err != nil {
		t.Fatal(err)
	}
	defer fresh.Close()
	if err = fresh.Import(ctx, old.Session); err != nil {
		t.Fatal(err)
	}
	got, err = fresh.Get(ctx, s.ID)
	if err != nil || !bytes.Equal(got.Credentials[0].Value, secret) {
		t.Fatalf("cold restore: %v", err)
	}
	delete(k.values, "second")
	if _, err = fresh.Get(ctx, s.ID); err == nil {
		t.Fatal("storage accepted missing key")
	}
}

func TestStorageRejectsCredentialsWithoutKeys(t *testing.T) {
	db, err := store.Open(t.TempDir(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	ctx := core.WithTenant(context.Background(), "one")
	s := core.Session{ID: core.ID(), Tenant: "one", CredentialBindings: []string{"data"}, Credentials: []credential.Record{{Binding: "data", Value: json.RawMessage(`"secret"`)}}}
	if err = db.Create(ctx, s); err == nil {
		t.Fatal("persisted plaintext without keys")
	}
	if _, err = db.Get(ctx, s.ID); core.ErrorCode(err) != core.NotFound {
		t.Fatalf("failed admission left metadata: %v", err)
	}
}
