// Package brain composes an agent session server with application-owned harnesses
// and tools. Session requests select installed implementations; they never install code.
package brain

import (
	"github.com/aexhq/brain/internal/config"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/persistence"
	"github.com/aexhq/brain/internal/tools"
)

type Config = config.Config
type Limits = config.Limits
type Token = config.Token
type ProcessConfig = config.Runner
type Persistence = persistence.Settings
type Grant = core.Grant
type Session = core.Session
type Event = core.Event
type ModelRequest = core.ModelRequest
type ModelUsage = core.ModelUsage
type SessionConfig = core.Config
type Tool = core.Tool
type ToolBinding = core.ToolBinding
type ToolScope = core.ToolScope
type Call = core.Call
type Result = core.Result
type Content = core.Content
type CreateSession = core.CreateSession
type CredentialInput = core.CredentialInput
type ModelCredential = core.ModelCredential
type CredentialKeys = credentials.Keys
type KeyConfig = config.KeyConfig
type ManagedTool = tools.Binding
type Handler = tools.Handler
type ToolRequest = tools.Request
type ToolProvider = tools.Provider
type ProviderUnavailable = tools.Unavailable
type BindingContext = tools.BindingContext
type HarnessFactory = harness.Factory
type Harness = harness.Runner
type HarnessOptions = harness.Options
type HarnessProvider = harness.Provider
type NativeFile = core.NativeFile
type ProcessFactory = harness.ProcessFactory
type HostPool = harness.HostPool
type HostPoolLimits = harness.PoolLimits

func NewHostPool(limits HostPoolLimits) (*HostPool, error) { return harness.NewHostPool(limits) }
func Codex(command []string) *ProcessFactory               { return harness.Codex(command) }
func OpenCode(command []string) *ProcessFactory            { return harness.OpenCode(command) }

func DefaultConfig() Config                         { return config.Default() }
func LoadConfig(file string) (Config, error)        { return config.Load(file) }
func ToolError(message string, unknown bool) Result { return core.ToolError(message, unknown) }
