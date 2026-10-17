// A Brain application with a trusted, session-bound function provider.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"os"
	"os/signal"
	"syscall"

	"github.com/aexhq/brain"
)

func main() {
	cfg, err := brain.LoadConfig("brain.json")
	if err != nil {
		log.Fatal(err)
	}
	greeting := brain.ToolProvider{
		Parameters: json.RawMessage(`{"type":"object","properties":{"prefix":{"type":"string"}},"required":["prefix"],"additionalProperties":false}`),
		Authorize: func(ctx context.Context, b brain.BindingContext) error {
			if b.Tenant != "local" {
				return errors.New("tenant is not authorized")
			}
			return nil
		},
		Bind: func(ctx context.Context, b brain.BindingContext) ([]brain.ManagedTool, error) {
			var params struct {
				Prefix string `json:"prefix"`
			}
			if err := json.Unmarshal(b.Parameters, &params); err != nil {
				return nil, err
			}
			return []brain.ManagedTool{{Tool: brain.Tool{Name: "hello", Description: "Greet a person", InputSchema: json.RawMessage(`{"type":"object","properties":{"name":{"type":"string"}},"required":["name"],"additionalProperties":false}`), OutputSchema: json.RawMessage(`{"type":"object","required":["text"]}`)}, Handler: func(ctx context.Context, r brain.ToolRequest, progress func(json.RawMessage)) brain.Result {
				var args struct {
					Name string `json:"name"`
				}
				if err := json.Unmarshal(r.Arguments, &args); err != nil {
					return brain.ToolError("invalid name", false)
				}
				text := params.Prefix + args.Name
				data, err := json.Marshal(map[string]string{"text": text})
				if err != nil {
					return brain.ToolError("encoding failed", false)
				}
				return brain.Result{Content: []brain.Content{{Type: "text", Text: text}}, StructuredContent: data}
			}}}, nil
		},
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	server, err := brain.New(ctx, brain.Options{Config: cfg, Providers: map[string]brain.ToolProvider{"greetings": greeting}})
	if err != nil {
		log.Fatal(err)
	}
	if err := server.Run(ctx); err != nil {
		log.Fatal(err)
	}
}
