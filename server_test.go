package brain_test

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestSeparateGoApplication(t *testing.T) {
	root, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	module := "module brain-consumer\n\ngo 1.26.0\n\nrequire github.com/aexhq/brain v0.0.0\nreplace github.com/aexhq/brain => " + filepath.ToSlash(root) + "\n"
	if err := os.WriteFile(filepath.Join(dir, "go.mod"), []byte(module), 0600); err != nil {
		t.Fatal(err)
	}
	source, err := os.ReadFile("testdata/application/main.go")
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(source), "/internal/") {
		t.Fatal("consumer imports internal API")
	}
	if err := os.WriteFile(filepath.Join(dir, "main.go"), source, 0600); err != nil {
		t.Fatal(err)
	}
	command := exec.Command("go", "run", "-mod=mod", ".")
	command.Dir = dir
	command.Env = append(os.Environ(), "GOWORK=off", "OTEL_TRACES_EXPORTER=none", "OTEL_METRICS_EXPORTER=none")
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("external application: %v\n%s", err, output)
	}
}
