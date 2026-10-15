package packages

import (
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func valid() wire.PackageManifest {
	return wire.PackageManifest{Name: "test-package", Version: "1", Language: wire.RuntimeLanguageNode, Entrypoint: "action.ts",
		Files:    map[string]string{"action.ts": "export default () => true", "package.json": `{"name":"test-package","version":"1.0.0"}`},
		Lockfile: `{"lockfileVersion":3,"packages":{"":{}}}`, Actions: []wire.Action{{Name: "call", Connection: "worker",
			InputSchema: kernel.JSON(map[string]string{"type": "object"}), MaxOutputBytes: 8192, TimeoutMs: 10000}},
		MemoryBytes: 64 << 20, CpuMillis: 1000, WallMs: 10000}
}

func TestManagedManifestBoundaries(t *testing.T) {
	if err := Validate(valid()); err != nil {
		t.Fatal(err)
	}
	cases := map[string]func(*wire.PackageManifest){
		"traversal":    func(m *wire.PackageManifest) { m.Files["../escape"] = "x" },
		"windows-path": func(m *wire.PackageManifest) { m.Files[`C:\escape`] = "x" },
		"build-script": func(m *wire.PackageManifest) { m.Files["Dockerfile"] = "RUN false" },
		"npm-config":   func(m *wire.PackageManifest) { m.Files[".npmrc"] = "registry=http://localhost" },
		"unlocked": func(m *wire.PackageManifest) {
			m.Lockfile = `{"lockfileVersion":3,"packages":{"node_modules/foo":{"resolved":"https://example.com/foo"}}}`
		},
		"link": func(m *wire.PackageManifest) {
			m.Lockfile = `{"lockfileVersion":3,"packages":{"node_modules/foo":{"link":true}}}`
		},
		"runtime-secret":  func(m *wire.PackageManifest) { m.Secrets = []string{"NODE_OPTIONS"} },
		"unbounded-wall":  func(m *wire.PackageManifest) { m.WallMs = 300001 },
		"too-much-memory": func(m *wire.PackageManifest) { m.MemoryBytes = 8 << 30 },
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			manifest := valid()
			change(&manifest)
			if Validate(manifest) == nil {
				t.Fatal("invalid package was admitted")
			}
		})
	}
}

func TestPythonRequiresHashedWheels(t *testing.T) {
	m := valid()
	m.Language, m.Entrypoint, m.Files = wire.RuntimeLanguagePython, "action.py", map[string]string{"action.py": "def main(args, ctx): return True"}
	m.Lockfile = "tomli==2.2.1 --hash=sha256:cb55c73c5f4408779d0cf3eef9f762b9c9f147a77de7b258bef0a5628adc85cc\n"
	if err := Validate(m); err != nil {
		t.Fatal(err)
	}
	for _, lockfile := range []string{"tomli>=2", "tomli==2.2.1", "--extra-index-url https://example.com", "-e .", "https://example.com/package.whl"} {
		m.Lockfile = lockfile
		if Validate(m) == nil {
			t.Fatalf("unlocked dependency accepted: %s", lockfile)
		}
	}
}
