package tools

import (
	"context"
	"encoding/json"
	"github.com/aexhq/brain/internal/core"
	"testing"
	"time"
)

func TestRuntimeBridgeScopeLossAndNoReplay(t *testing.T) {
	b := NewBridge(4)
	defer b.Close()
	tool := core.Tool{Name: "lookup", Description: "Lookup", InputSchema: json.RawMessage(`{"type":"object"}`)}
	credential, err := b.Register("one", ExecutorRegistration{Binding: "runtime", Tools: []core.Tool{tool}}, nil)
	if err != nil {
		t.Fatal(err)
	}
	s := core.Session{ID: core.ID(), Tenant: "one", Config: core.Config{Bindings: []core.ToolBinding{{Alias: "runtime", Executor: credential.ID, Runtime: []core.Tool{tool}}}}}
	if _, err := b.Resolve(s, s.Config.Bindings[0]); err == nil {
		t.Fatal("disconnected registration admitted")
	}
	c, err := b.Attach(credential.ID)
	if err != nil {
		t.Fatal(err)
	}
	bindings, err := b.Resolve(s, s.Config.Bindings[0])
	if err != nil {
		t.Fatal(err)
	}
	s.Descriptors = []core.Tool{tool}
	wrong := s
	wrong.Tenant = "two"
	if _, err := b.Resolve(wrong, s.Config.Bindings[0]); err == nil {
		t.Fatal("cross-tenant executor accepted")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	result := make(chan core.Result, 1)
	call := func(id string) {
		result <- bindings[0].Handler(ctx, Request{ToolScope: core.ToolScope{Tenant: "one", Session: s.ID, Call: id}, Arguments: json.RawMessage(`{}`)}, nil)
	}
	go call("first")
	command := <-c.Commands
	if command.Tool != "lookup" || command.Scope.Session != s.ID {
		t.Fatal(command)
	}
	if err := b.Reply(credential.ID, Reply{Connection: "stale", CallID: "first", Result: &core.Result{Content: []core.Content{{Type: "text", Text: "wrong"}}}}); err == nil {
		t.Fatal("stale result accepted")
	}
	b.Detach(credential.ID, c)
	if got := <-result; !got.IsError || !got.UnknownOutcome {
		t.Fatal(got)
	}
	go call("unavailable")
	if got := <-result; !got.IsError || got.UnknownOutcome {
		t.Fatal(got)
	}
	c, err = b.Attach(credential.ID)
	if err != nil {
		t.Fatal(err)
	}
	select {
	case <-c.Commands:
		t.Fatal("action replayed on reconnect")
	default:
	}
	go call("second")
	<-c.Commands
	reply := Reply{Connection: c.ID, CallID: "second", Result: &core.Result{Content: []core.Content{{Type: "text", Text: "ok"}}}}
	if err := b.Reply(credential.ID, reply); err != nil {
		t.Fatal(err)
	}
	if got := <-result; got.IsError {
		t.Fatal(got)
	}
	if err := b.Reply(credential.ID, reply); err == nil {
		t.Fatal("duplicate result accepted")
	}
	b.Detach(credential.ID, c)
	rebound, err := b.Register("one", ExecutorRegistration{Session: s.ID, Binding: "runtime", Tools: []core.Tool{tool}}, &s)
	if err != nil {
		t.Fatal(err)
	}
	if rebound.ID != credential.ID || rebound.Token == credential.Token {
		t.Fatal("invalid rebind identity")
	}
	if _, _, ok := b.Authenticate(credential.Token); ok {
		t.Fatal("old executor credential survived rebind")
	}
}
