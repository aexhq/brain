package sqlcontrol

import (
	"context"
	"fmt"
	"strings"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestDocumentReplacementAccountsForUTF8Bytes(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	value := kernel.JSON(strings.Repeat("界", 21844) + "xx")
	if len(value) != 65536 {
		t.Fatalf("fixture has %d bytes", len(value))
	}
	tx, err := store.db.BeginTx(ctx, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()
	for i := 0; i < 64; i++ {
		_, err = tx.ExecContext(ctx, store.q("INSERT INTO documents(session_id,scope,key,revision,value,request_key,request_hash) VALUES(?,'memory',?,1,?,'fixture','')"),
			session.View.ID, fmt.Sprint(i), string(value))
		if err != nil {
			t.Fatal(err)
		}
	}
	if err = tx.Commit(); err != nil {
		t.Fatal(err)
	}
	update := wire.DocumentUpdate{RequestID: "replace", Revision: "1", Value: value}
	out, err := store.PutDocument(ctx, session.View.ID, "memory", "0", update)
	if err != nil || out.Revision != "2" {
		t.Fatalf("replacement at exact byte quota: %+v %v", out, err)
	}
	update = wire.DocumentUpdate{RequestID: "new-key", Revision: "0", Value: kernel.JSON(0)}
	if _, err = store.PutDocument(ctx, session.View.ID, "state", "new", update); kernel.Response(err).Code != wire.ErrorCodeOverloaded {
		t.Fatalf("UTF-8 quota allowed an additional document: %v", err)
	}
}

func TestReplacingAtDocumentAndFileCountLimits(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	tx, err := store.db.BeginTx(ctx, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()
	for i := 0; i < 1024; i++ {
		key := fmt.Sprint(i)
		if _, err = tx.ExecContext(ctx, store.q("INSERT INTO documents(session_id,scope,key,revision,value,request_key,request_hash) VALUES(?,'state',?,1,'null','fixture','')"),
			session.View.ID, key); err != nil {
			t.Fatal(err)
		}
		if _, err = tx.ExecContext(ctx, store.q("INSERT INTO files(session_id,name,digest,size,media_type,provenance) VALUES(?,?,?,1048576,'application/octet-stream','fixture')"),
			session.View.ID, key, kernel.Digest([]byte(key))); err != nil {
			t.Fatal(err)
		}
	}
	if err = tx.Commit(); err != nil {
		t.Fatal(err)
	}
	if _, err = store.PutDocument(ctx, session.View.ID, "state", "0", wire.DocumentUpdate{RequestID: "replacement", Revision: "1", Value: kernel.JSON("updated")}); err != nil {
		t.Fatalf("document replacement at count quota: %v", err)
	}
	if _, err = store.PutDocument(ctx, session.View.ID, "memory", "new", wire.DocumentUpdate{RequestID: "new", Revision: "0", Value: kernel.JSON(0)}); kernel.Response(err).Code != wire.ErrorCodeOverloaded {
		t.Fatalf("document count quota not enforced: %v", err)
	}
	file := wire.FileInfo{Name: "0", Digest: kernel.Digest([]byte("replacement")), Size: 1048576, MediaType: "application/octet-stream", Provenance: "fixture"}
	if err = store.PutFile(ctx, session.View.ID, file); err != nil {
		t.Fatalf("file replacement at count and byte quotas: %v", err)
	}
	file.Name = "new"
	file.Size = 1
	if err = store.PutFile(ctx, session.View.ID, file); kernel.Response(err).Code != wire.ErrorCodeOverloaded {
		t.Fatalf("file count quota not enforced: %v", err)
	}
}
