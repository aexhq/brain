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
	Plugins      []PiPlugin                                            `json:"plugins,omitempty"`
	MediaOrigins []string                                              `json:"media_origins,omitempty"`
	Lifetime     context.Context                                       `json:"-"`
	SessionID    string                                                `json:"session_id"`
	Dir          string                                                `json:"dir"`
	Config       core.Config                                           `json:"config"`
	Tools        []core.Tool                                           `json:"tools"`
	Provider     Provider                                              `json:"provider"`
	Event        func(string, json.RawMessage) error                   `json:"-"`
	Call         func(context.Context, core.Call) (core.Result, error) `json:"-"`
}

type Runner interface {
	Prompt(context.Context, core.Text) error
	Steer(context.Context, core.Text) error
	Interrupt(context.Context) error
	Checkpoint(context.Context) ([]core.NativeFile, error)
	Close() error
}

// PiPlugin is installed by trusted server code, never by a session request.
type PiPlugin struct {
	Name   string          `json:"name"`
	Config json.RawMessage `json:"config,omitempty"`
	Tools  []string        `json:"tools,omitempty"`
}

type NativeTools interface {
	NativeTools() []core.Tool
	InvokeNative(context.Context, core.Call, func(json.RawMessage)) (core.Result, error)
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

func OpenCode(command []string) *ProcessFactory {
	return &ProcessFactory{Command: command, MaxShared: 8, MaxActiveTurns: 8, Validate: func(config core.Config, provider Provider) error {
		if provider.API != "" && provider.API != "openai-responses" {
			return core.E(core.Unsupported, "OpenCode requires the Responses transport")
		}
		if config.ReasoningEffort != "" && config.ReasoningEffort != "off" {
			return core.E(core.Unsupported, "OpenCode reasoning effort is not configured")
		}
		for _, name := range config.Tools {
			if name == "execute" {
				return core.E(core.Unsupported, "OpenCode reserves the execute tool name")
			}
		}
		return nil
	}}
}
