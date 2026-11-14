// Package api defines the canonical tool request, independent of execution mode.
package api

import (
	"encoding/json"
	core "github.com/aexhq/brain/services/brain/api"
)

type Kind string

const (
	HostedTool  Kind = "hosted"
	RemoteTool  Kind = "remote"
	BrowserTool Kind = "browser"
)

type Request struct {
	Operation   string                  `json:"operation"`
	Tenant      string                  `json:"tenant"`
	Session     core.Session            `json:"session"`
	Credentials []core.CredentialRecord `json:"credentials,omitempty"`
	Scope       core.ToolScope          `json:"scope"`
	Call        core.Call               `json:"call"`
}
type Frame struct {
	Tools    []core.Tool     `json:"tools,omitempty"`
	Session  *core.Session   `json:"session,omitempty"`
	Progress json.RawMessage `json:"progress,omitempty"`
	Result   *core.Result    `json:"result,omitempty"`
	Error    *core.Error     `json:"error,omitempty"`
	Done     bool            `json:"done,omitempty"`
}
type Upload struct {
	Tool   core.Tool `json:"tool"`
	Module string    `json:"module"`
}
type Package struct {
	Reference string    `json:"reference"`
	Tool      core.Tool `json:"tool"`
}
