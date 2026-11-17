package api

import (
	"context"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	"time"
)

type Artifact struct {
	Path       string `json:"path"`
	Generation string `json:"generation"`
	Size       int64  `json:"size"`
	Tail       string `json:"tail,omitempty"`
}

type Manifest struct {
	Credentials  []credential.Record `json:"credentials,omitempty"`
	ID           string              `json:"id"`
	Tenant       string              `json:"tenant"`
	Session      core.Session        `json:"session"`
	HistoryBytes int64               `json:"history_bytes"`
	Files        []Artifact          `json:"files"`
	Time         time.Time           `json:"time"`
}

type Snapshot struct {
	Session      core.Session
	Directory    string
	HistoryBytes int64
}

type Checkpoints interface {
	Delete(context.Context, string, string) error
	Publish(context.Context, Snapshot) (Manifest, error)
	Restore(context.Context, string, string, string, string) (Manifest, string, error)
	List(context.Context, string, string, int) ([]core.Session, error)
}
