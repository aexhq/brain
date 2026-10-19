// Package sessiontools is an example platform-owned session inspection policy.
// Copy it into the application that owns target authorization and discovery.
package sessiontools

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain"
)

type SessionReader interface {
	ReadSession(context.Context, string, string) (brain.Session, error)
	ReadEvents(context.Context, string, string, string, int) ([]brain.Event, error)
}

// Sessions grants only explicit, application-authorized target reads. The
// caller's tenant always comes from the authenticated binding, never arguments.
func Sessions(reader SessionReader, authorize func(context.Context, brain.BindingContext, string) error) brain.ToolProvider {
	return brain.ToolProvider{Parameters: json.RawMessage(`{"type":"object","additionalProperties":false}`), Authorize: func(context.Context, brain.BindingContext) error {
		if reader == nil || authorize == nil {
			return errors.New("session reader and target authorization are required")
		}
		return nil
	}, Bind: func(ctx context.Context, b brain.BindingContext) ([]brain.ManagedTool, error) {
		return []brain.ManagedTool{{Tool: brain.Tool{Name: "read", Description: "Read an authorized session and a bounded page of its public events", InputSchema: json.RawMessage(`{"type":"object","properties":{"session":{"type":"string","pattern":"^[a-f0-9]{32}$"},"after":{"type":"string"}},"required":["session"],"additionalProperties":false}`)}, Handler: func(ctx context.Context, r brain.ToolRequest, progress func(json.RawMessage)) brain.Result {
			var input struct {
				Session string `json:"session"`
				After   string `json:"after"`
			}
			if err := json.Unmarshal(r.Arguments, &input); err != nil {
				return brain.ToolError("invalid session read", false)
			}
			if err := authorize(ctx, b, input.Session); err != nil {
				return brain.ToolError("target session is not authorized", false)
			}
			session, err := reader.ReadSession(ctx, b.Tenant, input.Session)
			if err != nil {
				return brain.ToolError("target session is unavailable", false)
			}
			events, err := reader.ReadEvents(ctx, b.Tenant, input.Session, input.After, 100)
			if err != nil {
				return brain.ToolError("target history is unavailable", false)
			}
			data, err := json.Marshal(struct {
				Session brain.Session `json:"session"`
				Events  []brain.Event `json:"events"`
			}{session, events})
			if err != nil {
				return brain.ToolError("session read failed", false)
			}
			return brain.Result{Content: []brain.Content{{Type: "text", Text: string(data)}}, StructuredContent: data}
		}}}, nil
	}}
}
