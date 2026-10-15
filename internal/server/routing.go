package server

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/history"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Server) validateInternalURL(origin string) error {
	u, err := url.Parse(origin)
	if err != nil || u.User != nil || u.Host == "" || u.Path != "" || u.RawQuery != "" || u.Fragment != "" ||
		(u.Scheme != "https" && !(s.Config.Development && u.Scheme == "http")) {
		return kernel.Fail(wire.ErrorCodeInvalid, "internal node origin must use HTTPS or explicit development HTTP")
	}
	return nil
}

func (s *Server) internalRecord(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	err := func() error {
		stamp := r.Header.Get("X-Brain-Request-Time")
		when, err := strconv.ParseInt(stamp, 10, 64)
		if err != nil || when < time.Now().Add(-time.Minute).UnixMilli() || when > time.Now().Add(time.Minute).UnixMilli() ||
			!s.verify("internal-history", r.URL.RequestURI()+"\x00"+stamp, r.Header.Get("X-Brain-Node-Signature")) {
			return kernel.Fail(wire.ErrorCodeUnauthorized, "a current cluster read signature is required")
		}
		session, err := s.Control.Session(r.Context(), "", r.PathValue("session"))
		if err != nil {
			return err
		}
		if session.View.Lifecycle == wire.LifecycleDeleted {
			return kernel.Fail(wire.ErrorCodeNotFound, "deleted history cannot be routed")
		}
		sequence, err := kernel.Sequence(r.PathValue("sequence"))
		if err != nil {
			return err
		}
		allocation, err := s.Control.LocateRecord(r.Context(), session.View.ID, sequence)
		if err != nil {
			return err
		}
		if allocation.Status != "written" || allocation.Digest != r.URL.Query().Get("digest") {
			return kernel.Fail(wire.ErrorCodeConflict, "internal read does not match a written authoritative record")
		}
		record, err := s.History.Local(r.Context(), session.View.ID, sequence, allocation.Digest)
		if err != nil {
			return err
		}
		return writeJSON(w, record)
	}()
	if err != nil {
		writeError(w, err)
	}
}

func (s *Server) remoteRecord(ctx context.Context, origin, session string, sequence int64, digest string) (wire.Record, error) {
	var record wire.Record
	if err := s.validateInternalURL(origin); err != nil {
		return record, err
	}
	s.routeMu.Lock()
	until := s.routeFailures[origin]
	s.routeMu.Unlock()
	if time.Now().Before(until) {
		return record, kernel.Fail(wire.ErrorCodeUnavailable, "record node is in bounded read backoff")
	}
	path := "/internal/v2/records/" + url.PathEscape(session) + "/" + strconv.FormatInt(sequence, 10) + "?digest=" + url.QueryEscape(digest)
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, strings.TrimRight(origin, "/")+path, nil)
	if err != nil {
		return record, err
	}
	stamp := strconv.FormatInt(time.Now().UnixMilli(), 10)
	request.Header.Set("X-Brain-Request-Time", stamp)
	request.Header.Set("X-Brain-Node-Signature", s.sign("internal-history", path+"\x00"+stamp))
	client := &http.Client{Timeout: 3 * time.Second, Transport: &http.Transport{DisableKeepAlives: true},
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := client.Do(request)
	if err != nil {
		s.routeMu.Lock()
		for endpoint, expiry := range s.routeFailures {
			if time.Now().After(expiry) {
				delete(s.routeFailures, endpoint)
			}
		}
		if len(s.routeFailures) < 128 {
			s.routeFailures[origin] = time.Now().Add(time.Second)
		}
		s.routeMu.Unlock()
		return record, kernel.Fail(wire.ErrorCodeUnavailable, "record's previous node is unreachable")
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return record, kernel.Fail(wire.ErrorCodeUnavailable, "record is not available at its previous node")
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, history.MaxFrame+2))
	if err != nil || len(body) > history.MaxFrame+1 {
		return record, kernel.Fail(wire.ErrorCodeCorrupt, "routed record exceeds its bound or was truncated")
	}
	if err = json.Unmarshal(body, &record); err != nil {
		return record, kernel.Fail(wire.ErrorCodeCorrupt, "routed record is invalid JSON")
	}
	return record, nil
}
