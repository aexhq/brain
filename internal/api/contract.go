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
		{Name: "create", Method: http.MethodPost, Path: "/sessions", Input: reflect.TypeFor[core.CreateSession](), Output: reflect.TypeFor[core.Session](), Status: 201, Summary: "Create a session", Description: "Creates a session for the caller's tenant with the given harness, model, instructions, reasoning effort, and tools. Requires a management token. Creating a session records its metadata locally; the harness process starts when the first message is sent.", handler: s.create},
		{Name: "list", Method: http.MethodGet, Path: "/sessions", Output: reflect.TypeFor[SessionList](), Status: 200, Summary: "List sessions", Description: "Lists sessions belonging to the caller's tenant in ascending ID order. Pass the last session ID as after to fetch the next page.", handler: s.list},
		{Name: "get", Method: http.MethodGet, Path: "/sessions/{id}", Output: reflect.TypeFor[core.Session](), Status: 200, Summary: "Get a session", Description: "Returns metadata and current status for a session. If the session is not on local disk and external storage is configured, Brain loads the latest published checkpoint.", handler: s.get},
		{Name: "send", Method: http.MethodPost, Path: "/sessions/{id}/events", Input: reflect.TypeFor[core.Input](), Output: reflect.TypeFor[core.Event](), Status: 202, Summary: "Send input", Description: "Queues a user.message (with text and an optional idempotency key) or requests an interruption with user.interrupt. Returns 202 Accepted once the input is recorded locally.", handler: s.send},
		{Name: "events", Method: http.MethodGet, Path: "/sessions/{id}/events", Output: reflect.TypeFor[EventList](), Status: 200, Summary: "Read or stream events", Description: "Returns a page of retained events as JSON, or streams history and live updates when called with Accept: text/event-stream. Pass after (or Last-Event-ID) with the last retained event's cursor to resume from a known position. Returns 409 cursor_unavailable if the cursor points to an unsynced tail lost during restore.", handler: s.events},
		{Name: "resume", Method: http.MethodPost, Path: "/sessions/{id}/resume", Output: reflect.TypeFor[core.Session](), Status: 200, Summary: "Resume a session", Description: "Moves an interrupted or failed session back to idle from its last completed checkpoint. Requires a management token. Send a new user.message after resuming to continue the conversation.", handler: s.resume},
		{Name: "close", Method: http.MethodPost, Path: "/sessions/{id}/close", Output: reflect.TypeFor[core.Status](), Status: 202, Summary: "Close a session", Description: "Permanently closes a session and stops any active or queued work. Requires a management token. Event history remains readable.", handler: s.close},
		{Name: "flush", Method: http.MethodPost, Path: "/sessions/{id}/flush", Output: reflect.TypeFor[core.SyncStatus](), Status: 200, Summary: "Flush external state", Description: "Uploads local event history and the latest completed checkpoint to external storage and returns the resulting checkpoint reference. Requires a management token and configured persistence.", handler: s.flush},
		{Name: "sync", Method: http.MethodGet, Path: "/sessions/{id}/sync", Output: reflect.TypeFor[core.SyncStatus](), Status: 200, Summary: "Read synchronization status", Description: "Returns the latest published checkpoint, pending upload state, and any sync error for the session. Requires configured persistence.", handler: s.syncStatus},
		{Name: "restore", Method: http.MethodPost, Path: "/sessions/{id}/restore", Input: reflect.TypeFor[core.Restore](), Output: reflect.TypeFor[core.Session](), Status: 200, Summary: "Restore a checkpoint", Description: "Replaces local session state with a specific checkpoint from external storage. Requires a management token and settled local execution.", handler: s.restore},
		{Name: "catalog", Method: http.MethodGet, Path: "/catalog", Output: reflect.TypeFor[ToolCatalog](), Status: 200, Summary: "List available tools", Description: "Lists tool descriptors available to the caller's tenant. Endpoint URLs and credentials are omitted.", handler: func(w http.ResponseWriter, r *http.Request, g core.Grant) {
			write(w, 200, ToolCatalog{Tools: s.Tools.Catalog(g.Tenant)})
		}},
	}
}

func Contract() []Route { return (&Server{}).routes() }
