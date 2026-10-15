//go:build cloud

package persistence

import (
	"context"
	"encoding/json"
	"io"
	"os"
	"testing"
)

// The operator supplies an isolated bucket/prefix and cleans it up after the
// run. Missing credentials/configuration fail this explicitly invoked profile.
func TestCloudStorage(t *testing.T) {
	file := os.Getenv("BRAIN_TEST_STORAGE_CONFIG")
	if file == "" {
		t.Fatal("BRAIN_TEST_STORAGE_CONFIG must name a storage settings JSON file")
	}
	b, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	var settings Settings
	if err := json.Unmarshal(b, &settings); err != nil {
		t.Fatal(err)
	}
	if settings.Provider == "filesystem" || settings.Prefix == "" {
		t.Fatal("live qualification requires a cloud provider and isolated prefix")
	}
	objects, err := NewObjects(context.Background(), settings)
	if err != nil {
		t.Fatal(err)
	}
	if closer, ok := objects.(io.Closer); ok {
		t.Cleanup(func() { _ = closer.Close() })
	}
	objectContract(t, objects)
	archive, recorded, snapshot, log := fixture(t)
	recorded.Objects = objects
	archiveContract(t, archive, recorded, snapshot, log)
}
