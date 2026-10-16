// Package core contains the public session and tool data shared by adapters.
package core

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"
)

type Code string

const (
	Invalid           Code = "invalid_request"
	Forbidden         Code = "forbidden"
	NotFound          Code = "not_found"
	Conflict          Code = "conflict"
	Unsupported       Code = "unsupported"
	Overloaded        Code = "overloaded"
	CursorUnavailable Code = "cursor_unavailable"
	Internal          Code = "brain_error"
	ProviderError     Code = "provider_error"
)

type Error struct {
	Code    Code   `json:"code"`
	Message string `json:"message"`
}

func (e *Error) Error() string          { return string(e.Code) + ": " + e.Message }
func E(code Code, message string) error { return &Error{Code: code, Message: message} }
func ErrorCode(err error) Code {
	var e *Error
	if errors.As(err, &e) {
		return e.Code
	}
	return Internal
}

// ID is opaque and independent of a log position, including after tail loss.
func ID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic(err)
	}
	return hex.EncodeToString(b[:])
}

func ValidID(s string) bool {
	if len(s) != 32 {
		return false
	}
	_, err := hex.DecodeString(s)
	return err == nil
}

type Config struct {
	Model           string   `json:"model"`
	ReasoningEffort string   `json:"reasoning_effort,omitempty"`
	Instructions    string   `json:"instructions,omitempty"`
	Tools           []string `json:"tools"`
}

type CreateSession struct {
	Harness string `json:"harness"`
	Config  Config `json:"config"`
}

type Session struct {
	ID         string      `json:"id"`
	Tenant     string      `json:"-"`
	Harness    string      `json:"harness"`
	Config     Config      `json:"config"`
	Status     string      `json:"status"`
	CreatedAt  time.Time   `json:"created_at"`
	UpdatedAt  time.Time   `json:"updated_at"`
	Checkpoint *Checkpoint `json:"checkpoint,omitempty"`
}

type Event struct {
	ID      string          `json:"id"`
	Type    string          `json:"type"`
	Time    time.Time       `json:"time"`
	InputID string          `json:"input_id,omitempty"`
	Data    json.RawMessage `json:"data"`
	Cursor  string          `json:"cursor,omitempty"`
}

func NewEvent(kind, inputID string, data any) (Event, error) {
	b, err := json.Marshal(data)
	if err != nil {
		return Event{}, fmt.Errorf("encode %s event: %w", kind, err)
	}
	return Event{ID: ID(), Type: kind, Time: time.Now().UTC(), InputID: inputID, Data: b}, nil
}

type Input struct {
	Type     string `json:"type"`
	Text     string `json:"text,omitempty"`
	Delivery string `json:"delivery,omitempty"`
	Key      string `json:"key,omitempty"`
}

type Text struct {
	Text string `json:"text"`
}
type Status struct {
	Status string `json:"status"`
}

type Tool struct {
	Name         string          `json:"name"`
	Description  string          `json:"description"`
	InputSchema  json.RawMessage `json:"input_schema"`
	OutputSchema json.RawMessage `json:"output_schema,omitempty"`
}

type Call struct {
	ID        string          `json:"id"`
	Name      string          `json:"name"`
	Arguments json.RawMessage `json:"arguments"`
}

type Content struct {
	Type     string `json:"type"`
	Text     string `json:"text,omitempty"`
	Data     string `json:"data,omitempty"`
	MimeType string `json:"mime_type,omitempty"`
}

type Result struct {
	Content        []Content `json:"content"`
	IsError        bool      `json:"is_error,omitempty"`
	UnknownOutcome bool      `json:"unknown_outcome,omitempty"`
}

func ToolError(message string, unknown bool) Result {
	return Result{Content: []Content{{Type: "text", Text: message}}, IsError: true, UnknownOutcome: unknown}
}

type ToolResult struct {
	CallID string `json:"call_id"`
	Result Result `json:"result"`
}

type NativeFile struct {
	Path       string `json:"path"`
	Size       int64  `json:"size"`
	AppendOnly bool   `json:"append_only"`
}

type Checkpoint struct {
	ID     string       `json:"id"`
	Offset int64        `json:"offset"`
	Files  []NativeFile `json:"files,omitempty" wire:"-"`
	Config Config       `json:"config,omitzero" wire:"-"`
	Time   time.Time    `json:"time"`
}

type Restore struct {
	Checkpoint string `json:"checkpoint"`
}
type SyncStatus struct {
	Checkpoint       string    `json:"checkpoint,omitempty"`
	HistoryBytes     int64     `json:"history_bytes"`
	ResumeOffset     int64     `json:"resume_offset"`
	NativeCheckpoint string    `json:"native_checkpoint,omitempty"`
	Time             time.Time `json:"time,omitempty"`
	Pending          bool      `json:"pending"`
	Error            string    `json:"error,omitempty"`
}

type Grant struct {
	Tenant   string `json:"tenant"`
	Session  string `json:"session,omitempty"`
	Manage   bool   `json:"manage"`
	ReadOnly bool   `json:"read_only,omitempty"`
}

func (g Grant) Allows(s Session) bool {
	return g.Tenant == s.Tenant && (g.Session == "" || g.Session == s.ID)
}

// ToolScope is authenticated host context, never model-provided arguments.
type ToolScope struct {
	Tenant          string `json:"tenant_id"`
	Session         string `json:"session_id"`
	Call            string `json:"call_id"`
	InspectionToken string `json:"inspection_token,omitempty"`
	BrainURL        string `json:"brain_url,omitempty"`
}
