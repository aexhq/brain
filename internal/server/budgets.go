package server

import (
	"net/http"

	"github.com/aexhq/brain/internal/kernel"
)

func (s *Server) budget(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	value, err := s.Control.Budget(r.Context(), r.PathValue("session"))
	if err == nil {
		return writeJSON(w, value)
	}
	return err
}
