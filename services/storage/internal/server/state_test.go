package server_test

import (
	"archive/tar"
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"github.com/aexhq/brain/services/storage"
	"github.com/aexhq/brain/services/storage/api/credential"
	"io"
	"strings"
	"testing"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	workspace "github.com/aexhq/brain/services/storage/client"
	"github.com/aexhq/brain/services/storage/internal/adapters/memory"
)

func stateArchive(t *testing.T, content string) []byte {
	t.Helper()
	var b bytes.Buffer
	w := tar.NewWriter(&b)
	if err := w.WriteHeader(&tar.Header{Name: "native/state", Size: int64(len(content)), Mode: 0600}); err != nil {
		t.Fatal(err)
	}
	if _, err := io.WriteString(w, content); err != nil {
		t.Fatal(err)
	}
	if err := w.Close(); err != nil {
		t.Fatal(err)
	}
	// Docker's tar stream can contain padding beyond the end marker.
	return append(b.Bytes(), make([]byte, 8192)...)
}

func TestRemoteLogReplayBoundaryAndStaleWriter(t *testing.T) {
	ctx := core.WithTenant(context.Background(), "tenant")
	client, _ := service(t, memory.New(), memory.New())
	w := &workspace.Workspace{Client: client, Owner: "brain", Epoch: 1, MaxBytes: 16 << 20}
	s := core.Session{ID: "log", Tenant: "tenant"}
	if err := w.Create(ctx, s); err != nil {
		t.Fatal(err)
	}
	first, err := w.OpenLog(ctx, s, w.MaxBytes)
	if err != nil {
		t.Fatal(err)
	}
	defer first.Close()
	stale, err := w.OpenLog(ctx, s, w.MaxBytes)
	if err != nil {
		t.Fatal(err)
	}
	defer stale.Close()
	e, _ := core.NewEvent("user.message", "", core.Input{Text: strings.Repeat("x", (2<<20)+1)})
	accepted, err := first.Append(e)
	if err != nil {
		t.Fatal(err)
	}
	sub, boundary := first.Subscribe(4, 4<<20)
	defer sub.Close()
	later, _ := core.NewEvent("user.message", "", core.Input{Text: "later"})
	later, err = first.Append(later)
	if err != nil {
		t.Fatal(err)
	}
	page, err := first.Read("", 10, boundary)
	if err != nil || len(page) != 1 || page[0].ID != accepted.ID {
		t.Fatalf("replay crossed boundary: %v %v", page, err)
	}
	select {
	case live := <-sub.Events:
		if live.ID != later.ID {
			t.Fatal(live)
		}
		sub.Consumed(live)
	default:
		t.Fatal("missing live delivery")
	}
	if _, err = stale.Append(e); core.ErrorCode(err) != core.Conflict {
		t.Fatalf("stale append: %v", err)
	}
	if _, err = first.Read("", 10, boundary-1); core.ErrorCode(err) != core.CursorUnavailable {
		t.Fatalf("non-record boundary: %v", err)
	}
}

func TestCaptureRequiresCompleteStateAndPublishesOnlyCoherentCheckpoint(t *testing.T) {
	ctx, cancel := context.WithTimeout(core.WithTenant(context.Background(), "tenant"), 10*time.Second)
	defer cancel()
	client, records := service(t, memory.New(), memory.New())
	t.Setenv("STORAGE_TEST_KEY", base64.StdEncoding.EncodeToString(make([]byte, 32)))
	records.Keys = &storage.KeyConfig{ActiveID: "test", Environments: map[string]string{"test": "STORAGE_TEST_KEY"}}
	w := &workspace.Workspace{Client: client, Owner: "brain", Epoch: 1, MaxBytes: 16 << 20}
	s := core.Session{ID: "state", Tenant: "tenant", Status: "idle", Checkpoint: &core.Checkpoint{ID: core.ID()}}
	s.CredentialBindings = []string{"model"}
	s.Credentials = []credential.Record{{Binding: "model", Value: json.RawMessage(`"private-test-state"`)}}
	if err := w.Create(ctx, s); err != nil {
		t.Fatal(err)
	}
	files := []core.NativeFile{{Path: "state", Size: 3}}
	if err := w.CaptureNative(ctx, s.ID, files, bytes.NewReader(stateArchive(t, "one"))); err != nil {
		t.Fatal(err)
	}
	var initial bytes.Buffer
	if err := w.ReadNative(ctx, s.ID, &initial); err != nil || initial.Len() != 0 {
		t.Fatalf("uncommitted state was visible: %v", err)
	}
	s.Checkpoint = &core.Checkpoint{ID: core.ID(), Files: files}
	if err := w.Save(ctx, s); err != nil {
		t.Fatal(err)
	}
	a := &workspace.Archive{Workspace: w}
	original, err := a.Publish(ctx, workspace.Snapshot{Session: s})
	if err != nil {
		t.Fatal(err)
	}
	if err = w.CaptureNative(ctx, s.ID, files, bytes.NewReader(stateArchive(t, "two"))); err != nil {
		t.Fatal(err)
	}
	// A crash between capture and checkpoint save must not change recovery.
	published, err := a.Publish(ctx, workspace.Snapshot{Session: s})
	if err != nil {
		t.Fatal(err)
	}
	if published.Session.Checkpoint.ID != original.Session.Checkpoint.ID {
		t.Fatal("checkpoint advanced without save")
	}
	var recovered bytes.Buffer
	if err = w.ReadNative(ctx, s.ID, &recovered); err != nil {
		t.Fatal(err)
	}
	tr := tar.NewReader(&recovered)
	if _, err = tr.Next(); err != nil {
		t.Fatal(err)
	}
	data, err := io.ReadAll(tr)
	if err != nil || string(data) != "one" {
		t.Fatalf("uncommitted capture leaked: %q %v", data, err)
	}
	for _, bad := range [][]core.NativeFile{{{Path: "missing", Size: 3}}, {{Path: "../escape", Size: 3}}, {{Path: "state", Size: 4}}, {{Path: "state", Size: 3}, {Path: "state", Size: 3}}} {
		if err = w.CaptureNative(ctx, s.ID, bad, bytes.NewReader(stateArchive(t, "one"))); core.ErrorCode(err) != core.Invalid {
			t.Fatalf("invalid capture accepted: %v %v", bad, err)
		}
	}
	// Restore selects an immutable reference and keeps current credentials.
	restored, ref, err := a.Restore(ctx, s.Tenant, s.ID, original.ID, "")
	if err != nil {
		t.Fatal(err)
	}
	if restored.Session.Tenant != s.Tenant || len(restored.Session.Credentials) != 1 || !bytes.Equal(restored.Session.Credentials[0].Value, s.Credentials[0].Value) {
		t.Fatal("restore lost private session identity or credentials")
	}
	if err = w.InstallRestore(ctx, restored.Session, ref); err != nil {
		t.Fatal(err)
	}
	if _, _, err = a.Restore(core.WithTenant(ctx, "other"), "other", s.ID, original.ID, ""); core.ErrorCode(err) != core.NotFound {
		t.Fatalf("cross-tenant restore: %v", err)
	}
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: "replacement", Epoch: 2}
	if _, err = records.Claim(ctx, i); err != nil {
		t.Fatal(err)
	}
	if err = w.CaptureNative(ctx, s.ID, files, bytes.NewReader(stateArchive(t, "one"))); core.ErrorCode(err) != core.Conflict {
		t.Fatalf("stale capture: %v", err)
	}
}
