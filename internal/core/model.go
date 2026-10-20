package core

// ModelUsage is a replacement snapshot for one request or physical attempt.
// Missing quantities are unknown. Inclusion rules describe the native numbers;
// consumers must not add reasoning to output or cache to inclusive input twice.
type ModelUsage struct {
	Availability     string `json:"availability"`
	Semantics        string `json:"semantics"`
	InputTokens      *int64 `json:"input_tokens,omitempty"`
	OutputTokens     *int64 `json:"output_tokens,omitempty"`
	CacheReadTokens  *int64 `json:"cache_read_tokens,omitempty"`
	CacheWriteTokens *int64 `json:"cache_write_tokens,omitempty"`
	ReasoningTokens  *int64 `json:"reasoning_tokens,omitempty"`
	InputCache       string `json:"input_cache"`
	OutputReasoning  string `json:"output_reasoning"`
}

// ModelRequest identifies a logical generation. AttemptID identifies a physical
// attempt when observable; an empty AttemptID is the logical request snapshot.
// Logical and attempt usage are alternative evidence, never additive charges.
type ModelRequest struct {
	RequestID         string     `json:"request_id"`
	AttemptID         string     `json:"attempt_id,omitempty"`
	Purpose           string     `json:"purpose"`
	Boundary          string     `json:"boundary"`
	Unmeasured        []string   `json:"unmeasured,omitempty"`
	State             string     `json:"state"`
	Harness           string     `json:"harness"`
	Provider          string     `json:"provider"`
	Model             string     `json:"model"`
	ProviderRequestID string     `json:"provider_request_id,omitempty"`
	Attempts          string     `json:"attempts"`
	Usage             ModelUsage `json:"usage"`
}

func UnknownUsage() ModelUsage {
	return ModelUsage{Availability: "unknown", Semantics: "snapshot", InputCache: "unknown", OutputReasoning: "unknown"}
}

func (r ModelRequest) Validate() error {
	bad := func() error { return E(Internal, "invalid model-request evidence") }
	if !ValidID(r.RequestID) || (r.AttemptID != "" && !ValidID(r.AttemptID)) || len(r.ProviderRequestID) > 1024 {
		return bad()
	}
	switch r.Purpose {
	case "primary", "compaction", "other":
	default:
		return bad()
	}
	if r.Boundary != "request" && r.Boundary != "native_turn" {
		return bad()
	}
	for _, gap := range r.Unmeasured {
		switch gap {
		case "generations", "compaction", "retries", "auxiliary":
		default:
			return bad()
		}
	}
	switch r.State {
	case "started", "completed", "failed", "cancelled", "unknown":
	default:
		return bad()
	}
	if r.Attempts != "observed" && r.Attempts != "unobserved" {
		return bad()
	}
	if r.AttemptID != "" && r.Attempts != "observed" {
		return bad()
	}
	u := r.Usage
	if u.Semantics != "snapshot" {
		return bad()
	}
	switch u.Availability {
	case "reported", "partial", "unknown":
	default:
		return bad()
	}
	for _, inclusion := range []string{u.InputCache, u.OutputReasoning} {
		switch inclusion {
		case "included", "excluded", "unknown":
		default:
			return bad()
		}
	}
	quantities := []*int64{u.InputTokens, u.OutputTokens, u.CacheReadTokens, u.CacheWriteTokens, u.ReasoningTokens}
	for _, quantity := range quantities {
		if quantity != nil && (*quantity < 0 || u.Availability == "unknown") {
			return bad()
		}
	}
	if u.Availability == "reported" && (u.InputTokens == nil || u.OutputTokens == nil) {
		return bad()
	}
	return nil
}
