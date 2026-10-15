package api

import (
	"net/http"
	"reflect"

	"github.com/aexhq/brain/internal/core"
)

// Route binds documentation and routing to the same operation declaration.
type Route struct {
	Name    string
	Method  string
	Path    string
	Input   reflect.Type
	handler scoped
}

func (s *Server) routes() []Route {
	return []Route{
		{"create", http.MethodPost, "/sessions", reflect.TypeFor[core.CreateSession](), s.create},
		{"list", http.MethodGet, "/sessions", nil, s.list},
		{"get", http.MethodGet, "/sessions/{id}", nil, s.get},
		{"send", http.MethodPost, "/sessions/{id}/events", reflect.TypeFor[core.Input](), s.send},
		{"events", http.MethodGet, "/sessions/{id}/events", nil, s.events},
		{"resume", http.MethodPost, "/sessions/{id}/resume", nil, s.resume},
		{"close", http.MethodPost, "/sessions/{id}/close", nil, s.close},
		{"flush", http.MethodPost, "/sessions/{id}/flush", nil, s.flush},
		{"sync", http.MethodGet, "/sessions/{id}/sync", nil, s.syncStatus},
		{"restore", http.MethodPost, "/sessions/{id}/restore", reflect.TypeFor[core.Restore](), s.restore},
		{"catalog", http.MethodGet, "/catalog", nil, func(w http.ResponseWriter, r *http.Request, g core.Grant) {
			write(w, 200, map[string]any{"tools": s.Tools.Catalog(g.Tenant)})
		}},
	}
}

func Contract() []Route { return (&Server{}).routes() }
