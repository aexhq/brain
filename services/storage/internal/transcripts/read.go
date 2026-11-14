// Package transcripts reads bounded event pages without restoring Pi state.
package transcripts

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"slices"
)

type Reader struct {
	Get func(context.Context, api.Identity, string) (api.Object, error)
}
type cursor struct {
	Offset int64  `json:"o"`
	ID     string `json:"i"`
}

func (r Reader) payload(ctx context.Context, i api.Identity, ref string, limit int64) ([]byte, error) {
	data := []byte{}
	stack := []string{ref}
	for len(stack) > 0 {
		ref = stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		o, err := r.Get(ctx, i, ref)
		if err != nil {
			return nil, err
		}
		if int64(len(data)+len(o.Data)) > limit || len(o.Data) > 0 && len(o.References) > 0 {
			return nil, errors.New("invalid transcript payload")
		}
		data = append(data, o.Data...)
		for n := len(o.References) - 1; n >= 0; n-- {
			stack = append(stack, o.References[n])
		}
	}
	return data, nil
}
func (r Reader) Read(ctx context.Context, i api.Identity, head api.Record, after string, limit int, tail bool) ([]core.Event, string, error) {
	if limit < 1 || limit > 1000 || tail && after != "" {
		return nil, "", core.E(core.Invalid, "invalid event page")
	}
	var c cursor
	if after != "" {
		b, err := base64.RawURLEncoding.DecodeString(after)
		if err != nil || json.Unmarshal(b, &c) != nil || c.Offset < 0 || !core.ValidID(c.ID) {
			return nil, "", core.E(core.CursorUnavailable, "invalid event cursor")
		}
	}
	nodes := make([]api.Transcript, 0, limit)
	ref, end := head.Transcript, head.TranscriptBytes
	found := after == ""
	for ref != "" {
		o, err := r.Get(ctx, i, ref)
		if err != nil {
			return nil, "", err
		}
		var n api.Transcript
		if json.Unmarshal(o.Data, &n) != nil || n.Offset < 0 || n.Size <= 0 || n.Size > 2<<20 || n.Offset+n.Size != end {
			return nil, "", errors.New("invalid transcript chain")
		}
		if after != "" && n.Offset <= c.Offset {
			b, err := r.payload(ctx, i, n.Payload, n.Size)
			if err != nil {
				return nil, "", err
			}
			var event core.Event
			if json.Unmarshal(b, &event) != nil || n.Offset != c.Offset || event.ID != c.ID {
				return nil, "", core.E(core.CursorUnavailable, "cursor is not retained")
			}
			found = true
			break
		}
		nodes = append(nodes, n)
		if tail && len(nodes) == limit {
			break
		}
		if len(nodes) > limit {
			copy(nodes, nodes[1:])
			nodes = nodes[:limit]
		}
		ref, end = n.Previous, n.Offset
	}
	if !found {
		return nil, "", core.E(core.CursorUnavailable, "cursor is not retained")
	}
	slices.Reverse(nodes)
	events := []core.Event{}
	next := after
	bytes := int64(0)
	for _, n := range nodes {
		if len(events) > 0 && bytes+n.Size > 2<<20 {
			break
		}
		b, err := r.payload(ctx, i, n.Payload, n.Size)
		if err != nil {
			return nil, "", err
		}
		var event core.Event
		if err = json.Unmarshal(b, &event); err != nil {
			return nil, "", err
		}
		encoded, _ := json.Marshal(cursor{n.Offset, event.ID})
		event.Cursor = base64.RawURLEncoding.EncodeToString(encoded)
		events = append(events, event)
		next = event.Cursor
		bytes += n.Size
	}
	return events, next, nil
}
