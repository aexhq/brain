package server_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage"
	"github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	workspace "github.com/aexhq/brain/services/storage/client"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aexhq/brain/services/storage/internal/adapters/memory"
	"testing"
)

func TestCredentialsEncryptedInMetadataAndSnapshots(t *testing.T) {
	ctx := core.WithTenant(context.Background(), "one")
	t.Setenv("STORAGE_KEY", base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{7}, 32)))
	keys := &storage.KeyConfig{ActiveID: "storage", Environments: map[string]string{"storage": "STORAGE_KEY"}}
	objects, heads := memory.New(), memory.New()
	client, records := service(t, objects, heads)
	records.Keys = keys
	w := &workspace.Workspace{Client: client, Owner: "brain", Epoch: 1, MaxBytes: 16 << 20}
	secret := json.RawMessage(`{"token":"private-distributed-secret"}`)
	s := core.Session{ID: "session", Tenant: "one", Status: "idle", Checkpoint: &core.Checkpoint{ID: core.ID()}, CredentialBindings: []string{"data"}, Credentials: []credential.Record{{Binding: "data", Value: secret}}}
	if err := w.Create(ctx, s); err != nil {
		t.Fatal(err)
	}
	archive := &workspace.Archive{Workspace: w}
	manifest, err := archive.Publish(ctx, workspace.Snapshot{Session: s})
	if err != nil {
		t.Fatal(err)
	}
	for _, backend := range []adapters.Adapter{records.Disk, heads, objects} {
		page, err := backend.List(ctx, "", "", 100)
		if err != nil {
			t.Fatal(err)
		}
		if len(page.Objects) == 0 {
			t.Fatal("expected persisted data")
		}
		for _, item := range page.Objects {
			obj, err := backend.Get(ctx, item.Key)
			if err != nil {
				t.Fatal(err)
			}
			if bytes.Contains(obj.Data, []byte("private-distributed-secret")) {
				t.Fatalf("plaintext persisted at %s", item.Key)
			}
		}
	}
	recovered, cold := service(t, objects, heads)
	cold.Keys = keys
	fresh := &workspace.Workspace{Client: recovered, Owner: "brain", Epoch: 1, MaxBytes: 16 << 20}
	got, err := fresh.Get(ctx, s.ID)
	if err != nil || !bytes.Equal(got.Credentials[0].Value, secret) {
		t.Fatalf("cold metadata read: %v", err)
	}
	result, err := recovered.Do(ctx, api.Request{Operation: "snapshot.read", Identity: api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: "brain", Epoch: 1}, Reference: manifest.ID})
	if err != nil || !bytes.Equal(result.Record.Credentials[0].Value, secret) {
		t.Fatalf("snapshot read: %v", err)
	}
	// Current revocation wins even when reading an older checkpoint.
	got.Credentials[0].Revoked = true
	got.Credentials[0].Value = nil
	if err = fresh.Save(ctx, got); err != nil {
		t.Fatal(err)
	}
	result, err = recovered.Do(ctx, api.Request{Operation: "snapshot.read", Identity: api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: "brain", Epoch: 1}, Reference: manifest.ID})
	if err != nil || !result.Record.Credentials[0].Revoked {
		t.Fatalf("snapshot undid revocation: %v", err)
	}
	records.Keys = nil
	if _, err = w.Get(ctx, s.ID); err == nil {
		t.Fatal("missing storage key accepted")
	}
}

// A remote write can commit while its acknowledgement is lost. Retrying must
// compare decrypted content because fresh encryption produces different bytes.
type lostAcknowledgement struct {
	adapters.Adapter
	lose bool
}

func (a *lostAcknowledgement) Put(ctx context.Context, key string, data []byte, c adapters.Condition) (adapters.Object, error) {
	result, err := a.Adapter.Put(ctx, key, data, c)
	if err == nil && a.lose {
		a.lose = false
		return adapters.Object{}, errors.New("lost acknowledgement")
	}
	return result, err
}
func TestCredentialReplicationRecoversLostAcknowledgement(t *testing.T) {
	ctx := context.Background()
	t.Setenv("STORAGE_KEY", base64.StdEncoding.EncodeToString(make([]byte, 32)))
	remote := &lostAcknowledgement{Adapter: memory.New(), lose: true}
	_, records := service(t, memory.New(), remote)
	records.Keys = &storage.KeyConfig{ActiveID: "test", Environments: map[string]string{"test": "STORAGE_KEY"}}
	i := api.Identity{Tenant: "one", Session: "session", Owner: "brain", Epoch: 1}
	r := api.Record{Tenant: i.Tenant, Session: core.Session{ID: i.Session, CredentialBindings: []string{"data"}}, Credentials: []credential.Record{{Binding: "data", Value: json.RawMessage(`"secret"`)}}}
	if _, err := records.Put(ctx, i, r, 0); err != nil {
		t.Fatal(err)
	}
	if _, err := records.Flush(ctx, i); err == nil {
		t.Fatal("expected lost acknowledgement")
	}
	if _, err := records.Flush(ctx, i); err != nil {
		t.Fatalf("cannot recover committed encrypted metadata: %v", err)
	}
}
