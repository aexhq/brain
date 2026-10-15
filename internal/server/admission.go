package server

import (
	"net/http"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Server) admitHTTP(w http.ResponseWriter, r *http.Request) (func(), bool) {
	if len(r.RequestURI) > 8192 {
		writeError(w, kernel.Fail(wire.ErrorCodeInvalid, "request URI exceeds its bound"))
		return nil, false
	}
	if r.URL.Path == "/health/live" || r.URL.Path == "/health/ready" {
		return func() {}, true
	}
	lane := s.requestSlots
	if r.Method == http.MethodGet && strings.HasSuffix(r.URL.Path, "/events") {
		lane = s.streamSlots
	} else if strings.HasPrefix(r.URL.Path, "/operator/") || strings.HasSuffix(r.URL.Path, "/receipt") ||
		(r.Method == http.MethodPost && strings.HasSuffix(r.URL.Path, "/usage")) {
		lane = s.receiptSlots
	} else if strings.Contains(r.URL.Path, "/files/") {
		lane = s.fileSlots
	}
	select {
	case lane <- struct{}{}:
		_ = http.NewResponseController(w).SetReadDeadline(time.Now().Add(30 * time.Second))
		return func() { <-lane }, true
	default:
		writeError(w, kernel.Fail(wire.ErrorCodeOverloaded, "node HTTP decoding or streaming capacity is occupied"))
		return nil, false
	}
}
