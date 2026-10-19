package session

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"sort"
	"strings"

	"github.com/aexhq/brain/internal/core"
)

type requestObservation struct {
	request core.ModelRequest
	input   string
}

func requestKey(r core.ModelRequest) string { return r.RequestID + "/" + r.AttemptID }

func (e *Engine) observeModel(a *entry, data json.RawMessage) error {
	var request core.ModelRequest
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		return errors.New("invalid model-request evidence")
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("invalid model-request evidence")
	}
	if err := request.Validate(); err != nil {
		return err
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	request.Harness = a.meta.Harness
	request.Model = a.meta.Config.Model
	request.Provider, _, _ = strings.Cut(request.Model, "/")
	key := requestKey(request)
	previous, seen := a.requests[key]
	currentJSON, _ := json.Marshal(request)
	previousJSON, _ := json.Marshal(previous.request)
	if seen && bytes.Equal(previousJSON, currentJSON) {
		return nil
	}
	if a.current == "" || (!seen && request.State != "started") || (seen && (previous.input != a.current || previous.request.Purpose != request.Purpose || previous.request.Boundary != request.Boundary || (previous.request.State != "started" && previous.request.State != request.State))) {
		return errors.New("invalid model-request lifecycle")
	}
	if _, err := e.emit(a, "model.request", a.current, request); err != nil {
		return err
	}
	a.requests[key] = requestObservation{request: request, input: a.current}
	return nil
}

// Reconcile unfinished dispatches exactly once after a crash or stopped turn.
// A missing terminal observation never becomes a zero-usage success.
func (e *Engine) settleModelRequests(a *entry, input string) (bool, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	keys := make([]string, 0)
	for key, observation := range a.requests {
		if observation.request.State == "started" && (input == "" || observation.input == input) {
			keys = append(keys, key)
		}
	}
	sort.Strings(keys)
	for _, key := range keys {
		observation := a.requests[key]
		observation.request.State = "unknown"
		if observation.request.Usage.Availability != "unknown" {
			observation.request.Usage.Availability = "partial"
		}
		if _, err := e.emit(a, "model.request", observation.input, observation.request); err != nil {
			return true, err
		}
		a.requests[key] = observation
	}
	return len(keys) > 0, nil
}
