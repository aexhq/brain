package store

import (
	"bytes"
	"encoding/json"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"io"
	"os"
	"path/filepath"
	"sync"
)

var UploadStateMu sync.RWMutex

func strictJSON(data []byte, v any) error {
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(v); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing stored JSON")
	}
	return nil
}

func ReadState(dir string) (*api.Manifest, error) {
	UploadStateMu.RLock()
	b, err := os.ReadFile(filepath.Join(dir, "uploaded.json"))
	UploadStateMu.RUnlock()
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var m api.Manifest
	if err := strictJSON(b, &m); err != nil {
		return nil, err
	}
	if (!core.ValidID(m.ID) && !core.ValidObjectReference(m.ID)) || !core.ValidSessionID(m.Session.ID) || m.Session.Checkpoint == nil || !core.ValidID(m.Session.Checkpoint.ID) || m.HistoryBytes < 0 {
		return nil, errors.New("invalid local upload state")
	}
	return &m, nil
}
