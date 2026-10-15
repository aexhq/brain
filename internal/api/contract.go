package api

import (
	"net/http"
	"reflect"

	"github.com/aexhq/brain/internal/core"
)

// Route binds documentation and routing to the same operation declaration.
type Route struct {
	Name        string
	Method      string
	Path        string
	Input       reflect.Type
	Output      reflect.Type
	Status      int
	Summary     string
	Description string
	handler     scoped
}

type SessionList struct {
	Sessions []core.Session `json:"sessions"`
}
type EventList struct {
	Events     []core.Event `json:"events"`
	NextCursor string       `json:"next_cursor"`
}
type ToolCatalog struct {
	Tools []core.Tool `json:"tools"`
}
type ErrorResponse struct {
	Error     core.Error `json:"error"`
	RequestID string     `json:"request_id"`
}

func (s *Server) routes() []Route {
	return []Route{
		{Name: "create", Method: http.MethodPost, Path: "/sessions", Input: reflect.TypeFor[core.CreateSession](), Output: reflect.TypeFor[core.Session](), Status: 201, Summary: "Create a session", Description: "Create a tenant-owned session with frozen configuration. Requires an unrestricted management grant. Creation does not call a model; the runner starts on first execution.", handler: s.create},
		{Name: "list", Method: http.MethodGet, Path: "/sessions", Output: reflect.TypeFor[SessionList](), Status: 200, Summary: "List sessions", Description: "List authorized session metadata in ascending ID order. Pass the last session ID as after for the next page. Optional PostgreSQL discovery is eventually consistent.", handler: s.list},
		{Name: "get", Method: http.MethodGet, Path: "/sessions/{id}", Output: reflect.TypeFor[core.Session](), Status: 200, Summary: "Get a session", Description: "Read authorized session metadata. If local metadata is absent, configured external storage can hydrate the latest complete checkpoint. Reading does not invoke inference.", handler: s.get},
		{Name: "send", Method: http.MethodPost, Path: "/sessions/{id}/events", Input: reflect.TypeFor[core.Input](), Output: reflect.TypeFor[core.Event](), Status: 202, Summary: "Send input", Description: "Submit user.message with text and optional key. Delivery defaults to queue; steering is unsupported in this preview. Acceptance records input locally, not model consumption or external durability. Alternatively send user.interrupt without text, key, or delivery; this returns a status acknowledgement. Tool actions are never automatically replayed.", handler: s.send},
		{Name: "events", Method: http.MethodGet, Path: "/sessions/{id}/events", Output: reflect.TypeFor[EventList](), Status: 200, Summary: "Read or stream events", Description: "Read a page of durable events as JSON, or send Accept: text/event-stream to replay history and observe live output. The after query takes precedence over Last-Event-ID. Durable SSE frames have id equal to the opaque event cursor; assistant.delta and tool.progress are provisional. Keepalives are comments. Slow observers disconnect and must reconnect explicitly; disconnecting does not cancel execution. An unavailable cursor returns 409.", handler: s.events},
		{Name: "resume", Method: http.MethodPost, Path: "/sessions/{id}/resume", Output: reflect.TypeFor[core.Session](), Status: 200, Summary: "Resume a session", Description: "Return an interrupted or failed session to idle using its last completed native checkpoint. Requires management access and settled execution. Resume alone does not call a model or replay tools. Closed sessions cannot resume.", handler: s.resume},
		{Name: "close", Method: http.MethodPost, Path: "/sessions/{id}/close", Output: reflect.TypeFor[core.Status](), Status: 202, Summary: "Close a session", Description: "Request terminal closure. Requires management access. Observe session.status for completion. History is retained; remote actions and resources are not rolled back or removed.", handler: s.close},
		{Name: "flush", Method: http.MethodPost, Path: "/sessions/{id}/flush", Output: reflect.TypeFor[core.SyncStatus](), Status: 200, Summary: "Flush external state", Description: "Publish local history and the latest completed native checkpoint. Requires management access and external storage. History may be newer than the resumable native boundary. Flush does not pause or invoke inference. Retain the immutable checkpoint reference for handoff.", handler: s.flush},
		{Name: "sync", Method: http.MethodGet, Path: "/sessions/{id}/sync", Output: reflect.TypeFor[core.SyncStatus](), Status: 200, Summary: "Read synchronization status", Description: "Report external publication, pending work, and synchronization errors. Requires configured external storage. A pending tail may be lost on machine failure.", handler: s.syncStatus},
		{Name: "restore", Method: http.MethodPost, Path: "/sessions/{id}/restore", Input: reflect.TypeFor[core.Restore](), Output: reflect.TypeFor[core.Session](), Status: 200, Summary: "Restore a checkpoint", Description: "Restore the immutable checkpoint reference returned by flush. Requires management access and external storage. Quiesce the previous owner; local active requests, streams, and execution must settle before cache replacement. Restore does not infer or undo remote effects. Resume explicitly before sending new input when interrupted.", handler: s.restore},
		{Name: "catalog", Method: http.MethodGet, Path: "/catalog", Output: reflect.TypeFor[ToolCatalog](), Status: 200, Summary: "List available tools", Description: "List descriptors authorized for the caller's tenant. Host endpoints and credentials are excluded. Select tool names when creating a session.", handler: func(w http.ResponseWriter, r *http.Request, g core.Grant) {
			write(w, 200, ToolCatalog{Tools: s.Tools.Catalog(g.Tenant)})
		}},
	}
}

func Contract() []Route { return (&Server{}).routes() }
