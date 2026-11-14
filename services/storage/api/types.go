// Package api owns Storage's service contract and client.
package api

import (
	core "github.com/aexhq/brain/services/brain/api"
)

const ChunkBytes = 64 << 10

type Identity struct {
	Collection string `json:"collection,omitempty"`
	Tenant     string `json:"tenant"`
	Session    string `json:"session"`
	Owner      string `json:"owner"`
	Epoch      uint64 `json:"epoch"`
}

// Record includes private recovery material. It is never returned by Brain's
// public API; only authenticated service peers may read Storage records.
type Record struct {
	Session         core.Session            `json:"session"`
	Tenant          string                  `json:"tenant"`
	Credentials     []core.CredentialRecord `json:"credentials,omitempty"`
	Owner           string                  `json:"owner"`
	Epoch           uint64                  `json:"epoch"`
	Revision        uint64                  `json:"revision"`
	Transcript      string                  `json:"transcript,omitempty"`
	TranscriptBytes int64                   `json:"transcript_bytes"`
	Snapshot        string                  `json:"snapshot,omitempty"`
	Attachments     string                  `json:"attachments,omitempty"`
	Deleted         bool                    `json:"deleted,omitempty"`
}

// Object forms an immutable, bounded DAG. References are committed before the
// object and the session metadata pointer, preserving readable remote snapshots.
type Object struct {
	Data       []byte   `json:"data"`
	References []string `json:"references,omitempty"`
}

type Request struct {
	Tail             bool     `json:"tail,omitempty"`
	Operation        string   `json:"operation"`
	Identity         Identity `json:"identity"`
	Record           *Record  `json:"record,omitempty"`
	ExpectedRevision uint64   `json:"expected_revision,omitempty"`
	Reference        string   `json:"reference,omitempty"`
	Object           *Object  `json:"object,omitempty"`
	After            string   `json:"after,omitempty"`
	Limit            int      `json:"limit,omitempty"`
}
type Response struct {
	Events    []core.Event     `json:"events,omitempty"`
	Status    *core.SyncStatus `json:"status,omitempty"`
	Record    *Record          `json:"record,omitempty"`
	Records   []Record         `json:"records,omitempty"`
	Reference string           `json:"reference,omitempty"`
	Object    *Object          `json:"object,omitempty"`
	Next      string           `json:"next,omitempty"`
	Error     *core.Error      `json:"error,omitempty"`
}
type Transcript struct {
	Previous string `json:"previous,omitempty"`
	Payload  string `json:"payload"`
	Offset   int64  `json:"offset"`
	Size     int64  `json:"size"`
}
