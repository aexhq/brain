// Package harness isolates native agent loops from the Brain coordinator.
package harness

import (
	"context"
	"encoding/json"

	"github.com/aexhq/brain/internal/core"
)

type Provider struct {
	APIKey  string `json:"api_key,omitempty"`
	BaseURL string `json:"base_url,omitempty"`
	API     string `json:"api,omitempty"`
}

type Options struct {
	Lifetime  context.Context                                       `json:"-"`
	SessionID string                                                `json:"session_id"`
	Dir       string                                                `json:"dir"`
	Config    core.Config                                           `json:"config"`
	Tools     []core.Tool                                           `json:"tools"`
	Provider  Provider                                              `json:"provider"`
	Event     func(string, json.RawMessage) error                   `json:"-"`
	Call      func(context.Context, core.Call) (core.Result, error) `json:"-"`
}

type Runner interface {
	Prompt(context.Context, string) error
	Steer(context.Context, string) error
	Interrupt(context.Context) error
	Checkpoint(context.Context) ([]core.NativeFile, error)
	Close() error
}

type Factory interface {
	Open(context.Context, Options) (Runner, error)
	SupportsSteering() bool
}

type Validator interface {
	ValidateSession(core.Config, Provider) error
}

func Codex(command []string) *ProcessFactory {
	return &ProcessFactory{Command: command, CredentialHosts: true, MaxShared: 8, MaxActiveTurns: 8, Validate: func(config core.Config, provider Provider) error {
		if provider.API != "" && provider.API != "openai-responses" {
			return core.E(core.Unsupported, "Codex requires the Responses transport")
		}
		switch config.ReasoningEffort {
		case "", "off", "minimal", "low", "medium", "high", "xhigh":
		default:
			return core.E(core.Unsupported, "unsupported Codex reasoning effort")
		}
		return nil
	}}
}
