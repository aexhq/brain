package compute

import (
	"archive/tar"
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/packages"
)

//go:embed runtime/*
var runtimes embed.FS

var imageDigest = regexp.MustCompile(`^(?:[A-Za-z0-9._:/-]+@)?sha256:[a-f0-9]{64}$`)

type Docker struct {
	Command     string
	NodeImage   string
	PythonImage string
	Key         []byte
	Trusted     bool
}

func (d *Docker) Validate() error {
	if !d.Trusted || len(d.Key) < 32 {
		return kernel.Fail(wire.ErrorCodeForbidden, "local compute requires explicit trusted-development mode and a retained worker key")
	}
	if !imageDigest.MatchString(d.NodeImage) || !imageDigest.MatchString(d.PythonImage) {
		return kernel.Fail(wire.ErrorCodeInvalid, "worker base images must be immutable SHA-256 references")
	}
	return nil
}

func (d *Docker) Ready(ctx context.Context) error {
	if err := d.Validate(); err != nil {
		return err
	}
	body, err := d.command(ctx, nil, nil, 4096, "info", "--format", "{{.OSType}}")
	if err != nil || strings.TrimSpace(string(body)) != "linux" {
		return errors.New("trusted local worker requires a responsive Linux Docker engine")
	}
	for _, image := range []string{d.NodeImage, d.PythonImage} {
		if _, err := d.command(ctx, nil, nil, 4096, "image", "inspect", image, "--format", "{{.Id}}"); err != nil {
			return errors.New("worker requires pre-pulled immutable base images")
		}
	}
	return nil
}

func (d *Docker) Run(ctx context.Context, invocation wire.ManagedInvocation) wire.Receipt {
	if err := packages.Validate(invocation.Package.Manifest); err != nil {
		return failed("package failed managed-worker validation")
	}
	if kernel.Digest(kernel.JSON(invocation.Package.Manifest)) != invocation.Package.Digest {
		return failed("package source digest does not match admission")
	}
	if len(invocation.Package.Manifest.Network) != 0 {
		return failed("trusted local runtime admits no application egress; use a network-policy-capable provider")
	}
	if invocation.Execution.Operation.Action == "build_package" {
		return d.build(ctx, invocation)
	}
	return d.invoke(ctx, invocation)
}

type boundedOutput struct {
	bytes.Buffer
	limit    int
	exceeded bool
}

func (b *boundedOutput) Write(p []byte) (int, error) {
	size := len(p)
	remaining := b.limit - b.Len()
	if len(p) > remaining {
		b.exceeded = true
		p = p[:max(0, remaining)]
	}
	_, _ = b.Buffer.Write(p)
	return size, nil
}

func (d *Docker) command(ctx context.Context, stdin io.Reader, environment []string, limit int, arguments ...string) ([]byte, error) {
	command := d.Command
	if command == "" {
		command = "docker"
	}
	process := exec.CommandContext(ctx, command, arguments...)
	process.Stdin = stdin
	process.Env = append(os.Environ(), environment...)
	process.WaitDelay = 5 * time.Second
	out := &boundedOutput{limit: limit}
	process.Stdout = out
	process.Stderr = io.Discard
	err := process.Run()
	if out.exceeded {
		return out.Bytes(), errors.New("worker command output exceeded its bound")
	}
	return out.Bytes(), err
}

func (d *Docker) name(operation string) string {
	mac := hmac.New(sha256.New, d.Key)
	_, _ = mac.Write([]byte(operation))
	return "brain-managed-" + hex.EncodeToString(mac.Sum(nil))[:32]
}

func (d *Docker) remove(id string) {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	_, _ = d.command(ctx, nil, nil, 4096, "container", "rm", "--force", id)
}

func (d *Docker) create(ctx context.Context, invocation wire.ManagedInvocation, image string, build bool, command []string) (string, error) {
	m := invocation.Package.Manifest
	memory, cpu := m.MemoryBytes, m.CpuMillis
	network := "none"
	if build {
		memory, cpu, network = max(memory, 512<<20), max(cpu, 1000), "bridge"
	}
	args := []string{"container", "create", "--name", d.name(invocation.Execution.Operation.ID),
		"--label", "dev.aex.brain.operation=" + invocation.Execution.Operation.ID,
		"--label", "dev.aex.brain.package=" + invocation.Package.Digest,
		"--memory", strconv.FormatUint(memory, 10), "--memory-swap", strconv.FormatUint(memory, 10),
		"--cpus", fmt.Sprintf("%.3f", float64(cpu)/1000), "--pids-limit", "128",
		"--cap-drop=ALL", "--security-opt=no-new-privileges", "--network", network,
		"--user", "65532:65532", "--workdir", "/work", "--init", "--interactive",
		"--tmpfs", "/scratch:rw,noexec,nosuid,nodev,size=67108864,mode=1777",
		"--env", "HOME=/scratch", "--env", "TMPDIR=/scratch", "--env", "NPM_CONFIG_CACHE=/scratch/npm"}
	environment := []string{}
	if !build {
		args = append(args, "--read-only")
		for _, name := range m.Secrets {
			value, exists := invocation.Secrets[name]
			if !exists {
				return "", errors.New("scoped package secret is absent")
			}
			args = append(args, "--env", name)
			environment = append(environment, name+"="+value)
		}
	}
	args = append(args, image)
	args = append(args, command...)
	body, err := d.command(ctx, nil, environment, 4096, args...)
	id := strings.TrimSpace(string(body))
	if err != nil || !regexp.MustCompile(`^[a-f0-9]{64}$`).MatchString(id) {
		return "", errors.New("container creation has no confirmed resource identity")
	}
	return id, nil
}

func (d *Docker) build(ctx context.Context, invocation wire.ManagedInvocation) wire.Receipt {
	m := invocation.Package.Manifest
	image := d.NodeImage
	command := []string{"npm", "ci", "--ignore-scripts", "--omit=dev", "--no-audit", "--fund=false",
		"--fetch-retries=0", "--fetch-timeout=30000", "--registry=https://registry.npmjs.org"}
	if m.Language == wire.RuntimeLanguagePython {
		image = d.PythonImage
		command = []string{"python", "-m", "pip", "install", "--disable-pip-version-check", "--no-input", "--retries=0", "--timeout=30",
			"--require-hashes", "--only-binary=:all:", "--no-cache-dir", "--target=/work/dependencies", "--index-url=https://pypi.org/simple", "-r", "requirements.txt"}
	}
	command = append([]string{"/usr/bin/timeout", "--signal=TERM", "--kill-after=5s", "240s"}, command...)
	id, err := d.create(ctx, invocation, image, true, command)
	if err != nil {
		return unknown("builder resource creation was not confirmed; it is not retried")
	}
	defer d.remove(id)
	contents, err := source(invocation.Package)
	if err != nil {
		return failed("could not prepare a bounded package source")
	}
	if _, err = d.command(ctx, bytes.NewReader(contents), nil, 4096, "container", "cp", "--archive", "-", id+":/"); err != nil {
		return failed("could not copy source into the isolated builder")
	}
	if _, err = d.command(ctx, nil, nil, 65536, "container", "start", "--attach", id); err != nil {
		return failed("isolated dependency build failed")
	}
	state, err := d.command(ctx, nil, nil, 4096, "container", "inspect", id, "--format", "{{.State.Running}} {{.State.ExitCode}}")
	if err != nil || strings.TrimSpace(string(state)) != "false 0" {
		return failed("isolated dependency build did not finish successfully")
	}
	body, err := d.command(ctx, nil, nil, 4096, "container", "commit",
		"--change", "LABEL dev.aex.brain.source="+invocation.Package.Digest, id)
	result := strings.TrimSpace(string(body))
	if err != nil || !imageDigest.MatchString(result) {
		return unknown("dependency build finished but its immutable image identity is unavailable")
	}
	return wire.Receipt{Outcome: wire.OutcomeCompleted, Result: kernel.JSON(wire.PackageBuildResult{
		PackageID: invocation.Package.ID, SourceDigest: invocation.Package.Digest, Image: result, Builder: "local_compute", BaseImage: image,
	})}
}

func source(pkg wire.Package) ([]byte, error) {
	var buffer bytes.Buffer
	writer := tar.NewWriter(&buffer)
	dirs := map[string]bool{}
	var directory func(string) error
	directory = func(name string) error {
		if dirs[name] || name == "" {
			return nil
		}
		if before, _, ok := strings.Cut(name, "/"); ok && before != name {
			index := strings.LastIndex(name, "/")
			if err := directory(name[:index]); err != nil {
				return err
			}
		}
		dirs[name] = true
		return writer.WriteHeader(&tar.Header{Name: name, Mode: 0755, Typeflag: tar.TypeDir, Uid: 65532, Gid: 65532})
	}
	write := func(name, content string) error {
		if err := directory(name[:strings.LastIndex(name, "/")]); err != nil {
			return err
		}
		if err := writer.WriteHeader(&tar.Header{Name: name, Size: int64(len(content)), Mode: 0644, Uid: 65532, Gid: 65532}); err != nil {
			return err
		}
		_, err := writer.Write([]byte(content))
		return err
	}
	names := make([]string, 0, len(pkg.Manifest.Files))
	for name := range pkg.Manifest.Files {
		names = append(names, name)
	}
	slices.Sort(names)
	for _, name := range names {
		if err := write("work/"+name, pkg.Manifest.Files[name]); err != nil {
			return nil, err
		}
	}
	lock := "package-lock.json"
	if pkg.Manifest.Language == wire.RuntimeLanguagePython {
		lock = "requirements.txt"
	}
	if err := write("work/"+lock, pkg.Manifest.Lockfile); err != nil {
		return nil, err
	}
	for _, name := range []string{"node.mjs", "python.py"} {
		body, err := runtimes.ReadFile("runtime/" + name)
		if err != nil {
			return nil, err
		}
		if err = write("opt/brain/"+name, string(body)); err != nil {
			return nil, err
		}
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	if buffer.Len() > 1<<20 {
		return nil, errors.New("source archive bound exceeded")
	}
	return buffer.Bytes(), nil
}

func (d *Docker) invoke(ctx context.Context, invocation wire.ManagedInvocation) wire.Receipt {
	pkg := invocation.Package
	if pkg.Status != "ready" || pkg.Image == nil || !imageDigest.MatchString(*pkg.Image) {
		return failed("package has no admitted immutable image")
	}
	metadata, err := d.command(ctx, nil, nil, 4096, "image", "inspect", *pkg.Image, "--format", `{{index .Config.Labels "dev.aex.brain.source"}}`)
	if err != nil || strings.TrimSpace(string(metadata)) != pkg.Digest {
		return failed("package image is unavailable on this worker or has different source provenance")
	}
	command := []string{"node", "/opt/brain/node.mjs"}
	if pkg.Manifest.Language == wire.RuntimeLanguagePython {
		command = []string{"python", "-B", "/opt/brain/python.py"}
	}
	duration := max(1, (pkg.Manifest.WallMs+999)/1000)
	command = append([]string{"/usr/bin/timeout", "--signal=TERM", "--kill-after=5s", strconv.FormatUint(duration, 10) + "s"}, command...)
	id, err := d.create(ctx, invocation, *pkg.Image, false, command)
	if err != nil {
		return unknown("runtime resource creation was not confirmed")
	}
	defer d.remove(id)
	input := kernel.JSON(map[string]any{"execution": invocation.Execution, "entrypoint": pkg.Manifest.Entrypoint})
	body, executionError := d.command(ctx, bytes.NewReader(input), nil, 262144, "container", "start", "--attach", "--interactive", id)
	var result struct {
		Protocol string          `json:"protocol"`
		Outcome  wire.Outcome    `json:"outcome"`
		Result   json.RawMessage `json:"result"`
		Error    *string         `json:"error"`
	}
	if executionError == nil && json.Unmarshal(body, &result) == nil && result.Protocol == "brain-managed-result/1" &&
		(result.Outcome == wire.OutcomeCompleted || result.Outcome == wire.OutcomeFailed) {
		if uint64(len(result.Result)) > invocation.Execution.MaxOutputBytes {
			result.Result, result.Error = nil, kernel.Ptr("completed result exceeded its admitted bound")
		}
		return wire.Receipt{Outcome: result.Outcome, Result: result.Result, Error: result.Error}
	}
	status, err := d.command(ctx, nil, nil, 4096, "container", "inspect", id, "--format", "{{.State.Running}} {{.State.ExitCode}}")
	if err == nil && strings.TrimSpace(string(status)) == "false 0" {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("managed process completed but its receipt bytes are unavailable")}
	}
	return unknown("managed process stopped without authoritative result evidence")
}

func failed(message string) wire.Receipt {
	return wire.Receipt{Outcome: wire.OutcomeFailed, Error: &message}
}

func unknown(message string) wire.Receipt {
	return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: &message}
}
