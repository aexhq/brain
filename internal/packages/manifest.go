package packages

import (
	"encoding/json"
	"path"
	"regexp"
	"slices"
	"strings"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/validation"
)

var secretName = regexp.MustCompile(`^[A-Z][A-Z0-9_]{0,95}$`)
var pythonRequirement = regexp.MustCompile(`^[A-Za-z0-9_.-]+==[A-Za-z0-9_.+!-]+(?:\s+--hash=sha256:[a-f0-9]{64})+$`)

func Validate(manifest wire.PackageManifest) error {
	if !kernel.Identifier(manifest.Name) || manifest.Version == "" || len(manifest.Version) > 128 ||
		len(manifest.Files) == 0 || len(manifest.Files) > 64 || len(kernel.JSON(manifest)) > 200000 ||
		manifest.MemoryBytes < 64<<20 || manifest.MemoryBytes > 4<<30 || manifest.CpuMillis < 100 ||
		manifest.CpuMillis > 4000 || manifest.WallMs < 100 || manifest.WallMs > 300000 || len(manifest.Secrets) > 32 {
		return kernel.Fail(wire.ErrorCodeInvalid, "package names, source and resource limits must satisfy the managed-worker bounds")
	}
	if !validPath(manifest.Entrypoint) || manifest.Files[manifest.Entrypoint] == "" {
		return kernel.Fail(wire.ErrorCodeInvalid, "entrypoint must name an uploaded source file")
	}
	for name := range manifest.Files {
		if !validPath(name) || name == "Dockerfile" || strings.HasPrefix(name, ".") || name == "package-lock.json" || name == "requirements.txt" || name == "node_modules" || strings.HasPrefix(name, "node_modules/") {
			return kernel.Fail(wire.ErrorCodeInvalid, "package source contains a reserved or unsafe path")
		}
	}
	for i, name := range manifest.Secrets {
		if !secretName.MatchString(name) || slices.Contains(manifest.Secrets[:i], name) ||
			strings.HasPrefix(name, "BRAIN_") || strings.HasPrefix(name, "LD_") || strings.HasPrefix(name, "NODE_") ||
			strings.HasPrefix(name, "PYTHON") || name == "PATH" || name == "HOME" {
			return kernel.Fail(wire.ErrorCodeInvalid, "package secret names must be unique non-runtime environment names")
		}
	}
	if len(manifest.Actions) == 0 || len(manifest.Actions) > 64 {
		return kernel.Fail(wire.ErrorCodeInvalid, "package must declare between one and 64 actions")
	}
	seen := map[string]bool{}
	for _, action := range manifest.Actions {
		if !kernel.Identifier(action.Name) || seen[action.Name] || action.Package != nil || action.MaxOutputBytes == 0 ||
			action.MaxOutputBytes > 196608 || action.TimeoutMs < manifest.WallMs || action.TimeoutMs > 300000 {
			return kernel.Fail(wire.ErrorCodeInvalid, "package actions need unique names and bounded output and deadlines")
		}
		seen[action.Name] = true
		if _, err := validation.Schema(action.InputSchema); err != nil {
			return err
		}
		if len(action.OutputSchema) > 0 {
			if _, err := validation.Schema(action.OutputSchema); err != nil {
				return err
			}
		}
	}
	switch manifest.Language {
	case wire.RuntimeLanguageNode:
		var lock struct {
			Version  int `json:"lockfileVersion"`
			Packages map[string]struct {
				Resolved  string            `json:"resolved"`
				Integrity string            `json:"integrity"`
				Link      bool              `json:"link"`
				Scripts   map[string]string `json:"scripts"`
			} `json:"packages"`
		}
		if json.Unmarshal([]byte(manifest.Lockfile), &lock) != nil || lock.Version != 3 || lock.Packages == nil || manifest.Files["package.json"] == "" {
			return kernel.Fail(wire.ErrorCodeInvalid, "Node packages require package.json and a version-three npm lockfile")
		}
		for name, item := range lock.Packages {
			if item.Link || (name != "" && (!strings.HasPrefix(item.Resolved, "https://registry.npmjs.org/") || !strings.HasPrefix(item.Integrity, "sha512-"))) {
				return kernel.Fail(wire.ErrorCodeInvalid, "Node dependencies require immutable npm registry URLs and SHA-512 integrity")
			}
		}
	case wire.RuntimeLanguagePython:
		if !strings.HasSuffix(manifest.Entrypoint, ".py") {
			return kernel.Fail(wire.ErrorCodeInvalid, "Python entrypoint must be a Python source file")
		}
		lines := strings.ReplaceAll(manifest.Lockfile, "\\\n", " ")
		for _, line := range strings.Split(lines, "\n") {
			line = strings.TrimSpace(line)
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}
			if !pythonRequirement.MatchString(line) {
				return kernel.Fail(wire.ErrorCodeInvalid, "Python dependencies require exact versions and SHA-256 wheel hashes")
			}
		}
	default:
		return kernel.Fail(wire.ErrorCodeUnsupported, "managed packages support Node/TypeScript and Python")
	}
	return nil
}

func validPath(name string) bool {
	return name != "" && len(name) <= 240 && name == path.Clean(name) && name != "." &&
		!strings.HasPrefix(name, "/") && !strings.HasPrefix(name, "../") && name != ".." &&
		!strings.ContainsAny(name, "\\:\x00\r\n")
}
