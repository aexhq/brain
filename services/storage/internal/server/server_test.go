package server_test

import (
	"context"
	"errors"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	workspace "github.com/aexhq/brain/services/brain/storage"
	"github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aexhq/brain/services/storage/internal/adapters/memory"
	"github.com/aexhq/brain/services/storage/internal/server"
	"github.com/aexhq/brain/services/storage/internal/sessions/metadata"
	"github.com/aexhq/brain/services/storage/internal/tiering"
)

func service(t *testing.T, objects, heads adapters.Adapter) (*api.Client, *metadata.Store) {
	t.Helper()
	cache, e := tiering.New(context.Background(), tiering.Options{Disk: memory.New(), Remote: objects, Index: filepath.Join(t.TempDir(), "cache.sqlite"), MemoryBytes: 1 << 20, DiskBytes: 16 << 20, MemoryTTL: time.Minute, DiskTTL: time.Hour, Interval: time.Hour})
	if e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() {
		if e := cache.Close(); e != nil {
			t.Error(e)
		}
	})
	s := &metadata.Store{Disk: memory.New(), Remote: heads, Objects: cache}
	token := strings.Repeat("x", 32)
	h, e := (&server.Server{Store: s, Token: token}).Handler()
	if e != nil {
		t.Fatal(e)
	}
	http := httptest.NewServer(h)
	t.Cleanup(http.Close)
	return &api.Client{URL: http.URL, Token: token}, s
}
func TestDistributedWorkspaceRecoveryFencingAndDeletion(t *testing.T) {
	ctx := core.WithTenant(context.Background(), "tenant-a")
	remote, heads := memory.New(), memory.New()
	client, records := service(t, remote, heads)
	w := &workspace.Workspace{Client: client, Owner: "owner-a", Epoch: 1, Directory: t.TempDir(), MaxBytes: 16 << 20}
	s := core.Session{ID: "customer/issue:42", Tenant: "tenant-a", Harness: "pi", Status: "idle", CreatedAt: time.Now().UTC(), UpdatedAt: time.Now().UTC(), Checkpoint: &core.Checkpoint{ID: core.ID(), Files: []core.NativeFile{}}}
	if e := w.Create(ctx, s); e != nil {
		t.Fatal(e)
	}
	l, e := w.OpenLog(ctx, s, 16<<20)
	if e != nil {
		t.Fatal(e)
	}
	event, _ := core.NewEvent("user.message", "", core.Input{Type: "user.message", Text: "retained input"})
	event, e = l.Append(event)
	if e != nil {
		t.Fatal(e)
	}
	size := l.Size()
	if e = l.Close(); e != nil {
		t.Fatal(e)
	}
	dir, _ := w.Dir(core.SessionKey(s.Tenant, s.ID))
	native := []byte("native Pi state\n")
	if e = os.WriteFile(filepath.Join(dir, "native", "session.jsonl"), native, 0600); e != nil {
		t.Fatal(e)
	}
	s.Checkpoint = &core.Checkpoint{ID: core.ID(), Offset: size, Files: []core.NativeFile{{Path: "session.jsonl", Size: int64(len(native)), AppendOnly: true}}}
	if e = w.Save(ctx, s); e != nil {
		t.Fatal(e)
	}
	a := &workspace.Archive{Workspace: w}
	manifest, e := a.Publish(ctx, workspace.Snapshot{Session: s, Directory: dir, HistoryBytes: size})
	if e != nil {
		t.Fatal(e)
	}
	if !core.ValidObjectReference(manifest.ID) {
		t.Fatal("flush did not return an immutable reference")
	}
	// Start with entirely empty local Storage and Brain caches. Only external
	// objects and the external session head survive this simulated host loss.
	recovered, _ := service(t, remote, heads)
	fresh := &workspace.Workspace{Client: recovered, Owner: "owner-a", Epoch: 1, Directory: t.TempDir(), MaxBytes: 16 << 20}
	got, e := fresh.Get(ctx, s.ID)
	if e != nil {
		t.Fatal(e)
	}
	log, e := fresh.OpenLog(ctx, got, 16<<20)
	if e != nil {
		t.Fatal(e)
	}
	history, e := log.Read("", 10, -1)
	if e != nil || len(history) != 1 || history[0].ID != event.ID {
		t.Fatalf("replay: %+v %v", history, e)
	}
	log.Close()
	dir, _ = fresh.Dir(core.SessionKey(s.Tenant, s.ID))
	data, e := os.ReadFile(filepath.Join(dir, "native", "session.jsonl"))
	if e != nil || string(data) != string(native) {
		t.Fatalf("native restore: %q %v", data, e)
	}
	listed, e := fresh.List(ctx, s.Tenant, "", 10)
	if e != nil || len(listed) != 1 || listed[0].ID != s.ID {
		t.Fatalf("cold discovery: %+v %v", listed, e)
	}
	identity := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: "owner-a", Epoch: 1}
	other := identity
	other.Tenant = "tenant-b"
	ref, e := client.PutObject(ctx, identity, api.Object{Data: []byte("same bytes")})
	if e != nil {
		t.Fatal(e)
	}
	otherRef, e := client.PutObject(ctx, other, api.Object{Data: []byte("same bytes")})
	if e != nil || ref != otherRef {
		t.Fatal(ref, otherRef, e)
	}
	if e = w.Delete(ctx, s); e != nil {
		t.Fatal(e)
	}
	if e = a.Delete(ctx, s.Tenant, s.ID); e != nil {
		t.Fatal(e)
	}
	if _, e = client.GetObject(ctx, identity, ref); core.ErrorCode(e) != core.NotFound {
		t.Fatal("deleted data survived", e)
	}
	if _, e = client.GetObject(ctx, other, otherRef); e != nil {
		t.Fatal("another tenant lost data", e)
	}
	if _, e = client.PutObject(ctx, identity, api.Object{Data: []byte("late write")}); core.ErrorCode(e) != core.NotFound {
		t.Fatal("deleted session accepted a late write", e)
	}
	// A new placement epoch invalidates the old owner before any metadata write.
	s.ID = "fenced"
	if e = w.Create(ctx, s); e != nil {
		t.Fatal(e)
	}
	i := api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: "next-owner", Epoch: 2}
	r, e := records.Claim(ctx, i)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = records.Put(ctx, api.Identity{Tenant: s.Tenant, Session: s.ID, Owner: "owner-a", Epoch: 1}, r, r.Revision); core.ErrorCode(e) != core.Conflict {
		t.Fatal("stale owner was accepted", e)
	}
}

type outage struct {
	adapters.Adapter
	fail bool
}

func (o *outage) Put(ctx context.Context, k string, b []byte, c adapters.Condition) (adapters.Object, error) {
	if o.fail {
		return adapters.Object{}, errors.New("object backend unavailable")
	}
	return o.Adapter.Put(ctx, k, b, c)
}
func TestHeadIsNotPublishedBeforeItsDependencies(t *testing.T) {
	ctx := context.Background()
	objects := &outage{Adapter: memory.New(), fail: true}
	heads := memory.New()
	client, s := service(t, objects, heads)
	i := api.Identity{Tenant: "a", Session: "publication", Owner: "owner", Epoch: 1}
	leaf, e := client.PutObject(ctx, i, api.Object{Data: []byte("state")})
	if e != nil {
		t.Fatal(e)
	}
	root, e := client.PutObject(ctx, i, api.Object{References: []string{leaf}})
	if e != nil {
		t.Fatal(e)
	}
	r := api.Record{Tenant: i.Tenant, Session: core.Session{ID: i.Session}, Snapshot: root}
	if _, e = s.Put(ctx, i, r, 0); e != nil {
		t.Fatal(e)
	}
	if _, e = s.Flush(ctx, i); e == nil {
		t.Fatal("flush ignored an object outage")
	}
	p, e := heads.List(ctx, metadata.Namespace(i.Tenant)+"/sessions/", "", 10)
	if e != nil || len(p.Objects) != 0 {
		t.Fatal("published an unreadable head", p, e)
	}
	objects.fail = false
	if _, e = s.Flush(ctx, i); e != nil {
		t.Fatal(e)
	}
	p, e = heads.List(ctx, metadata.Namespace(i.Tenant)+"/sessions/", "", 10)
	if e != nil || len(p.Objects) != 1 {
		t.Fatal(p, e)
	}
	bad := *client
	bad.Token = "wrong"
	if _, e = bad.Do(ctx, api.Request{Operation: "session.get", Identity: i}); e == nil {
		t.Fatal("unauthenticated storage access succeeded")
	}
}

func TestExternalIDsPageInOrderAndPackagesSurviveSessionDeletion(t *testing.T) {
	ctx := context.Background()
	client, records := service(t, memory.New(), memory.New())
	ids := []string{"z", "a/", "a", strings.Repeat("x", 64), strings.Repeat("x", 65), strings.Repeat("x", 128), "hosted-tools"}
	for _, id := range ids {
		i := api.Identity{Tenant: "tenant", Session: id, Owner: "brain", Epoch: 1}
		if _, err := records.Put(ctx, i, api.Record{Tenant: i.Tenant, Session: core.Session{ID: id}}, 0); err != nil {
			t.Fatal(err)
		}
		if _, err := records.Flush(ctx, i); err != nil {
			t.Fatal(err)
		}
	}
	slices.Sort(ids)
	got := []string{}
	after := ""
	for {
		page, next, err := records.List(ctx, "tenant", after, 2)
		if err != nil {
			t.Fatal(err)
		}
		for _, r := range page {
			got = append(got, r.Session.ID)
		}
		if next == "" {
			break
		}
		after = next
	}
	if !slices.Equal(got, ids) {
		t.Fatalf("IDs reordered or lost: %v", got)
	}
	i := api.Identity{Tenant: "tenant", Session: "hosted-tools", Collection: "packages"}
	ref, err := client.PutObject(ctx, i, api.Object{Data: []byte("uploaded package")})
	if err != nil {
		t.Fatal(err)
	}
	sessionID := api.Identity{Tenant: "tenant", Session: "hosted-tools", Owner: "brain", Epoch: 1}
	r, err := records.Get(ctx, sessionID)
	if err != nil {
		t.Fatal(err)
	}
	r.Deleted = true
	if _, err = records.Put(ctx, sessionID, r, r.Revision); err != nil {
		t.Fatal(err)
	}
	if err = records.Erase(ctx, sessionID); err != nil {
		t.Fatal(err)
	}
	if _, err = client.GetObject(ctx, i, ref); err != nil {
		t.Fatalf("session deletion removed package: %v", err)
	}
}

func TestAttachmentFinalizationAndTenantScope(t *testing.T) {
	ctx := core.WithTenant(context.Background(), "tenant")
	client, _ := service(t, memory.New(), memory.New())
	w := &workspace.Workspace{Client: client, Owner: "brain", Epoch: 1, Directory: t.TempDir(), MaxBytes: 16 << 20}
	s := core.Session{ID: "attachments", Tenant: "tenant", Harness: "pi", Status: "idle"}
	if err := w.Create(ctx, s); err != nil {
		t.Fatal(err)
	}
	i := api.Identity{Tenant: s.Tenant, Session: s.ID}
	unlinked, _, err := client.Upload(ctx, i, strings.NewReader("incomplete"), 100)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = w.Attachment(ctx, s.ID, unlinked); core.ErrorCode(err) != core.NotFound {
		t.Fatal("unlinked upload became addressable", err)
	}
	item, err := w.UploadAttachment(ctx, s.ID, "notes.txt", "text/plain", strings.NewReader("attached content"))
	if err != nil {
		t.Fatal(err)
	}
	resolved, err := w.ResolveInput(ctx, s.ID, core.Input{Text: "read", Attachments: []string{item.Reference}})
	if err != nil || !strings.Contains(resolved.Text, "attached content") {
		t.Fatal(resolved, err)
	}
	if _, err = w.Attachment(core.WithTenant(ctx, "other"), s.ID, item.Reference); core.ErrorCode(err) != core.NotFound {
		t.Fatal("cross-tenant attachment access", err)
	}
	if err = w.ValidateInput(ctx, s.ID, core.Input{References: []core.Reference{{URI: "http://127.0.0.1/private", MediaType: "text/plain"}}}); core.ErrorCode(err) != core.Forbidden {
		t.Fatal("unapproved reference origin accepted", err)
	}
}
