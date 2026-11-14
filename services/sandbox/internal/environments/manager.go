package environments

import (
	"archive/tar"
	"context"
	"errors"
	"fmt"
	"io"
	"math"
	"os/exec"
	"path"
	"strconv"
	"strings"
	"sync"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/sandbox/api"
)

type Profile struct {
	Image           string `json:"image"`
	Network         string `json:"network"`
	MemoryBytes     int64  `json:"memory_bytes"`
	CPUs            string `json:"cpus"`
	PIDs            int    `json:"pids"`
	WorkspaceBytes  int64  `json:"workspace_bytes"`
	LifetimeSeconds int    `json:"lifetime_seconds"`
}
type environment struct {
	value     api.Environment
	born      time.Time
	busy      bool
	cancel    context.CancelFunc
	removing  chan struct{}
	removeErr error
}
type Manager struct {
	Profiles     map[string]Profile
	Instance     string
	Capacity     int
	Docker       string
	mu           sync.Mutex
	environments map[string]*environment
	starting     int
	closed       bool
}

func (m *Manager) command(ctx context.Context, args ...string) *exec.Cmd {
	binary := m.Docker
	if binary == "" {
		binary = "docker"
	}
	return exec.CommandContext(ctx, binary, args...)
}
func (m *Manager) Validate() error {
	if m.Instance == "" || m.Capacity < 1 || len(m.Profiles) == 0 {
		return errors.New("sandbox instance, capacity and profiles are required")
	}
	for _, p := range m.Profiles {
		cpu, e := strconv.ParseFloat(p.CPUs, 64)
		if p.Image == "" || strings.HasPrefix(p.Image, "-") || p.Network == "" || p.MemoryBytes < 64<<20 || p.PIDs < 8 || p.WorkspaceBytes < 1<<20 || p.LifetimeSeconds < 1 || e != nil || cpu <= 0 || math.IsNaN(cpu) || math.IsInf(cpu, 0) {
			return errors.New("invalid sandbox profile")
		}
	}
	return nil
}
func (m *Manager) Create(ctx context.Context, in api.Create) (api.Environment, error) {
	if in.Tenant == "" || !core.ValidSessionID(in.Session) {
		return api.Environment{}, errors.New("sandbox scope is required")
	}
	p, ok := m.Profiles[in.Profile]
	if !ok {
		return api.Environment{}, errors.New("sandbox profile is not installed")
	}
	m.mu.Lock()
	if m.closed || len(m.environments)+m.starting >= m.Capacity {
		m.mu.Unlock()
		return api.Environment{}, core.E(core.Overloaded, "sandbox capacity reached")
	}
	m.starting++
	m.mu.Unlock()
	defer func() { m.mu.Lock(); m.starting--; m.mu.Unlock() }()
	id := "brain-env-" + core.ID()
	v := api.Environment{ID: id, Profile: in.Profile}
	args := []string{"run", "--detach", "--init", "--name", id, "--label", "brain.sandbox=" + m.Instance, "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges", "--user", "1000:1000", "--network", p.Network, "--memory", strconv.FormatInt(p.MemoryBytes, 10), "--memory-swap", strconv.FormatInt(p.MemoryBytes, 10), "--cpus", p.CPUs, "--pids-limit", strconv.Itoa(p.PIDs), "--tmpfs", "/tmp:rw,nosuid,nodev,size=67108864,uid=1000,gid=1000", "--tmpfs", fmt.Sprintf("/workspace:rw,nosuid,nodev,size=%d,uid=1000,gid=1000", p.WorkspaceBytes), "--workdir", "/workspace", "--env", "HOME=/workspace", "--env", "NO_COLOR=1", "--entrypoint", "sh", p.Image, "-c", "exec sleep infinity"}
	if e := m.command(ctx, args...).Run(); e != nil {
		c, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = m.command(c, "rm", "--force", id).Run()
		return v, fmt.Errorf("create sandbox environment: %w", e)
	}
	m.mu.Lock()
	if m.environments == nil {
		m.environments = map[string]*environment{}
	}
	m.environments[id] = &environment{value: v, born: time.Now()}
	closed := m.closed
	m.mu.Unlock()
	if closed {
		_ = m.Remove(context.Background(), id)
		return v, errors.New("sandbox is draining")
	}
	return v, nil
}
func (m *Manager) acquire(id string) (func(), error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	e := m.environments[id]
	if e == nil {
		return nil, core.E(core.NotFound, "environment not found")
	}
	if e.busy {
		return nil, core.E(core.Conflict, "environment has an active process or file operation")
	}
	e.busy = true
	return func() { m.mu.Lock(); e.busy = false; m.mu.Unlock() }, nil
}
func (m *Manager) Remove(ctx context.Context, id string) error {
	m.mu.Lock()
	e := m.environments[id]
	if e == nil {
		m.mu.Unlock()
		return nil
	}
	if done := e.removing; done != nil {
		m.mu.Unlock()
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-done:
		}
		m.mu.Lock()
		err := e.removeErr
		m.mu.Unlock()
		return err
	}
	e.removing = make(chan struct{})
	done := e.removing
	if e.cancel != nil {
		e.cancel()
	}
	m.mu.Unlock()
	err := m.command(ctx, "rm", "--force", id).Run()
	m.mu.Lock()
	e.removeErr = err
	if err == nil {
		delete(m.environments, id)
	} else {
		e.removing = nil
	}
	close(done)
	m.mu.Unlock()
	return err
}

// CopyTar accepts only regular files and directories rooted in /workspace.
// Links, device nodes and escaping names are rejected before extraction.
func CopyTar(w io.Writer, r io.Reader, limit int64) error {
	tr := tar.NewReader(io.LimitReader(r, limit+1))
	tw := tar.NewWriter(w)
	defer tw.Close()
	var total int64
	count := 0
	for {
		h, e := tr.Next()
		if e == io.EOF {
			return tw.Close()
		}
		if e != nil {
			return e
		}
		name := strings.TrimPrefix(h.Name, "./")
		if name == "." || name == "" {
			if h.Typeflag == tar.TypeDir {
				continue
			}
			return errors.New("invalid archive entry")
		}
		if path.Clean(name) != strings.TrimSuffix(name, "/") || strings.HasPrefix(name, "/") || strings.Contains(name, "\\") || name == ".." || strings.HasPrefix(name, "../") || (h.Typeflag != tar.TypeReg && h.Typeflag != tar.TypeDir) {
			return errors.New("invalid sandbox archive entry")
		}
		count++
		total += h.Size
		if count > 10000 || total > limit || h.Size < 0 {
			return errors.New("sandbox archive exceeds limit")
		}
		mode := int64(0600)
		if h.Typeflag == tar.TypeDir {
			mode = 0700
		}
		header := &tar.Header{Name: name, Typeflag: h.Typeflag, Size: h.Size, Mode: mode, Uid: 1000, Gid: 1000}
		if e = tw.WriteHeader(header); e != nil {
			return e
		}
		if _, e = io.CopyN(tw, tr, h.Size); e != nil {
			return e
		}
	}
}
func (m *Manager) Import(ctx context.Context, id string, r io.Reader, limit int64) error {
	release, e := m.acquire(id)
	if e != nil {
		return e
	}
	defer release()
	reader, writer := io.Pipe()
	done := make(chan error, 1)
	go func() { e := CopyTar(writer, r, limit); _ = writer.CloseWithError(e); done <- e }()
	cmd := m.command(ctx, "exec", "-i", id, "tar", "-xmf", "-", "-C", "/workspace", "--no-same-owner")
	cmd.Stdin = reader
	err := cmd.Run()
	_ = reader.CloseWithError(err)
	return errors.Join(err, <-done)
}
func (m *Manager) Export(ctx context.Context, id string, w io.Writer, limit int64) error {
	// A resident runtime can be idle while its stdio stream remains connected.
	// The caller must request a native checkpoint before exporting its files.
	m.mu.Lock()
	exists := m.environments[id] != nil
	m.mu.Unlock()
	if !exists {
		return core.E(core.NotFound, "environment not found")
	}
	cmd := m.command(ctx, "exec", id, "tar", "-cf", "-", "-C", "/workspace", ".")
	r, e := cmd.StdoutPipe()
	if e != nil {
		return e
	}
	if e = cmd.Start(); e != nil {
		return e
	}
	copyErr := CopyTar(w, r, limit)
	if copyErr != nil {
		_ = cmd.Process.Kill()
	}
	return errors.Join(copyErr, cmd.Wait())
}
func (m *Manager) Process(ctx context.Context, id string, command []string, stdin io.Reader, stdout io.Writer, started func(error)) error {
	if len(command) == 0 || len(command) > 128 || strings.HasPrefix(command[0], "-") {
		e := errors.New("invalid process command")
		started(e)
		return e
	}
	for _, a := range command {
		if len(a) > 64<<10 || strings.ContainsRune(a, 0) {
			e := errors.New("invalid process argument")
			started(e)
			return e
		}
	}
	release, e := m.acquire(id)
	if e != nil {
		started(e)
		return e
	}
	defer release()
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	m.mu.Lock()
	if e := m.environments[id]; e != nil && e.removing == nil {
		e.cancel = cancel
	} else {
		m.mu.Unlock()
		err := errors.New("environment is stopping")
		started(err)
		return err
	}
	m.mu.Unlock()
	args := append([]string{"exec", "-i", id}, command...)
	cmd := m.command(ctx, args...)
	cmd.Stdin = stdin
	cmd.Stdout = stdout
	cmd.WaitDelay = 2 * time.Second
	if e = cmd.Start(); e != nil {
		started(e)
		return e
	}
	started(nil)
	return cmd.Wait()
}
func (m *Manager) Reap(ctx context.Context) error {
	// Only containers labelled for this configured controller are recoverable
	// after its crash. No other Docker workload is inspected or removed.
	b, e := m.command(ctx, "ps", "--all", "--quiet", "--filter", "label=brain.sandbox="+m.Instance).Output()
	if e != nil {
		return e
	}
	for _, id := range strings.Fields(string(b)) {
		if e = m.command(ctx, "rm", "--force", id).Run(); e != nil {
			return e
		}
	}
	return nil
}
func (m *Manager) Run(ctx context.Context) {
	t := time.NewTicker(time.Second)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
		}
		m.mu.Lock()
		ids := []string{}
		for id, e := range m.environments {
			if time.Since(e.born) > time.Duration(m.Profiles[e.value.Profile].LifetimeSeconds)*time.Second {
				ids = append(ids, id)
			}
		}
		m.mu.Unlock()
		for _, id := range ids {
			c, cancel := context.WithTimeout(ctx, 15*time.Second)
			_ = m.Remove(c, id)
			cancel()
		}
	}
}
func (m *Manager) Close(ctx context.Context) error {
	m.mu.Lock()
	m.closed = true
	ids := []string{}
	for id := range m.environments {
		ids = append(ids, id)
	}
	m.mu.Unlock()
	var e error
	for _, id := range ids {
		e = errors.Join(e, m.Remove(ctx, id))
	}
	return e
}
