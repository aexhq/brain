package transcripts

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
)

func TestPagesTailAndCursorValidation(t *testing.T) {
	objects := map[string]api.Object{}
	head := api.Record{Snapshot: "must-not-read-native-state"}
	ids := []string{}
	for n := range 100 {
		e, err := core.NewEvent("assistant.message", "", core.Text{Text: fmt.Sprint(n)})
		if err != nil {
			t.Fatal(err)
		}
		ids = append(ids, e.ID)
		b, _ := json.Marshal(e)
		payload, ref := fmt.Sprintf("payload-%d", n), fmt.Sprintf("node-%d", n)
		objects[payload] = api.Object{Data: b}
		node, _ := json.Marshal(api.Transcript{Previous: head.Transcript, Payload: payload, Offset: head.TranscriptBytes, Size: int64(len(b))})
		objects[ref] = api.Object{Data: node}
		head.Transcript, head.TranscriptBytes = ref, head.TranscriptBytes+int64(len(b))
	}
	reads := 0
	r := Reader{Get: func(_ context.Context, _ api.Identity, key string) (api.Object, error) {
		reads++
		o, ok := objects[key]
		if !ok {
			t.Fatalf("unexpected object read: %s", key)
		}
		return o, nil
	}}
	ctx := context.Background()
	tail, _, err := r.Read(ctx, api.Identity{}, head, "", 3, true)
	if err != nil || len(tail) != 3 || tail[0].ID != ids[97] || reads != 6 {
		t.Fatalf("tail=%v reads=%d err=%v", tail, reads, err)
	}
	cursor := ""
	for start := 0; start < 100; start += 7 {
		page, next, err := r.Read(ctx, api.Identity{}, head, cursor, 7, false)
		if err != nil || len(page) != min(7, 100-start) {
			t.Fatalf("page at %d: %v %v", start, page, err)
		}
		for n, e := range page {
			if e.ID != ids[start+n] {
				t.Fatal("page reordered or duplicated events")
			}
		}
		cursor = next
	}
	page, next, err := r.Read(ctx, api.Identity{}, head, cursor, 7, false)
	if err != nil || len(page) != 0 || next != cursor {
		t.Fatalf("end page: %v %v", page, err)
	}
	if _, _, err = r.Read(ctx, api.Identity{}, head, "invalid", 7, false); core.ErrorCode(err) != core.CursorUnavailable {
		t.Fatal(err)
	}
	old, _ := json.Marshal(api.Transcript{Payload: "payload-0", Offset: 0, Size: 1})
	objects[head.Transcript] = api.Object{Data: old}
	if _, _, err = r.Read(ctx, api.Identity{}, head, "", 3, true); err == nil {
		t.Fatal("corrupt chain accepted")
	}
}
