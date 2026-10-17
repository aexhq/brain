package harness

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/aexhq/brain/internal/core"
)

const MaxFrame = 4 << 20

type ProcessFactory struct {
	Command        []string
	Steering       bool
	Logger         *slog.Logger
	MaxShared      int
	MaxActiveTurns int
	MaxHosts       int
	MaxStarting    int
	mu             sync.Mutex
	pool           []*process
	starting       chan struct{}
}

func (f *ProcessFactory) SupportsSteering() bool { return f.Steering }

type RunnerFrame struct {
	Attachment string          `json:"attachment"`
	Turn       string          `json:"turn,omitempty"`
	ID         string          `json:"id,omitempty"`
	Method     string          `json:"method,omitempty"`
	Params     json.RawMessage `json:"params,omitempty"`
	Result     json.RawMessage `json:"result,omitempty"`
	Error      *core.Error     `json:"error,omitempty"`
	Event      string          `json:"event,omitempty"`
	Data       json.RawMessage `json:"data,omitempty"`
}

type process struct {
	cmd         *exec.Cmd
	in          io.WriteCloser
	writeMu     sync.Mutex
	mu          sync.Mutex
	pending     map[string]chan RunnerFrame
	ctx         context.Context
	cancel      context.CancelFunc
	done        chan struct{}
	waited      chan struct{}
	readDone    chan struct{}
	callWG      sync.WaitGroup
	err         error
	once        sync.Once
	attachments map[string]*attachment
	active      chan struct{}
	directory   string
	killTree    func() error
	tools       chan struct{}
}

func (f *ProcessFactory) start(ctx context.Context, opts Options) (*process, error) {
	if len(f.Command) == 0 {
		return nil, core.E(core.Unsupported, "harness command is not configured")
	}
	owner := opts.Lifetime
	if owner == nil {
		owner = ctx
	}
	procCtx, cancel := context.WithCancel(owner)
	command := append([]string{}, f.Command...)
	for i := 1; i < len(command); i++ {
		if info, err := os.Stat(command[i]); err == nil && info.Mode().IsRegular() {
			command[i], err = filepath.Abs(command[i])
			if err != nil {
				cancel()
				return nil, err
			}
		}
	}
	directory, err := os.MkdirTemp("", "brain-host-")
	if err != nil {
		cancel()
		return nil, err
	}
	started := false
	defer func() {
		if !started {
			_ = os.RemoveAll(directory)
		}
	}()
	cmd := exec.CommandContext(procCtx, command[0], command[1:]...)
	cmd.Dir = directory
	// Inherit process necessities, not ambient credentials or harness settings.
	for _, key := range []string{"PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "TMP", "TEMP", "TMPDIR", "LANG", "LC_ALL"} {
		if value, ok := os.LookupEnv(key); ok {
			cmd.Env = append(cmd.Env, key+"="+value)
		}
	}
	cmd.Env = append(cmd.Env, "NO_COLOR=1", "HOME="+directory, "USERPROFILE="+directory)
	cmd.WaitDelay = 3 * time.Second
	in, err := cmd.StdinPipe()
	if err != nil {
		cancel()
		return nil, err
	}
	out, err := cmd.StdoutPipe()
	if err != nil {
		cancel()
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		cancel()
		return nil, err
	}
	p := &process{cmd: cmd, in: in, pending: make(map[string]chan RunnerFrame), ctx: procCtx, cancel: cancel, done: make(chan struct{}), waited: make(chan struct{}), readDone: make(chan struct{}), attachments: map[string]*attachment{}, active: make(chan struct{}, f.MaxActiveTurns), directory: directory, tools: make(chan struct{}, 32)}
	killTree, err := startOwned(cmd)
	if err != nil {
		cancel()
		return nil, err
	}
	p.killTree = killTree
	started = true
	go func() {
		s := bufio.NewScanner(stderr)
		s.Buffer(make([]byte, 4096), 64<<10)
		for s.Scan() {
			if f.Logger != nil {
				f.Logger.Debug("harness diagnostic", "session", opts.SessionID, "message", "runner diagnostic omitted")
			}
		}
	}()
	go p.read(out)
	go func() {
		err := cmd.Wait()
		if err == nil {
			err = errors.New("harness exited")
		}
		p.fail(err)
		close(p.waited)
	}()
	return p, nil
}

func (p *process) fail(err error) {
	p.once.Do(func() {
		p.mu.Lock()
		p.err = err
		p.mu.Unlock()
		close(p.done)
		p.cancel()
		if p.killTree != nil {
			_ = p.killTree()
		}
	})
}

func (p *process) read(r io.Reader) {
	defer close(p.readDone)
	s := bufio.NewScanner(r)
	s.Buffer(make([]byte, 64<<10), MaxFrame)
	for s.Scan() {
		var f RunnerFrame
		d := json.NewDecoder(strings.NewReader(s.Text()))
		d.DisallowUnknownFields()
		if err := d.Decode(&f); err != nil || d.Decode(new(any)) != io.EOF {
			p.fail(errors.New("invalid harness protocol RunnerFrame"))
			return
		}

		p.mu.Lock()
		a := p.attachments[f.Attachment]
		p.mu.Unlock()
		if a == nil {
			p.fail(errors.New("unknown harness session attachment"))
			return
		}
		a.mu.Lock()
		turn := a.turn
		a.mu.Unlock()
		if (f.Event != "" || f.Method == "tool.call") && (turn == "" || f.Turn != turn) {
			p.fail(errors.New("stale harness turn"))
			return
		}
		switch {
		case f.Event != "":
			if f.ID != "" || len(f.Data) == 0 {
				p.fail(errors.New("invalid harness event"))
				return
			}
			if err := a.opts.Event(f.Event, f.Data); err != nil {
				p.fail(err)
				return
			}
		case f.Method == "tool.call":
			var call core.Call
			if f.ID == "" || json.Unmarshal(f.Params, &call) != nil || call.Name == "" || call.ID == "" {
				p.fail(errors.New("invalid harness tool call"))
				return
			}
			select {
			case p.tools <- struct{}{}:
			default:
				p.fail(errors.New("harness exceeded concurrent tool limit"))
				return
			}
			p.callWG.Add(1)
			a.calls.Add(1)
			go func(f RunnerFrame, call core.Call) {
				defer p.callWG.Done()
				defer a.calls.Done()
				defer func() { <-p.tools }()
				result, err := a.opts.Call(p.ctx, call)
				if err != nil {
					if err := p.write(RunnerFrame{Attachment: f.Attachment, Turn: f.Turn, ID: f.ID, Error: &core.Error{Code: core.Internal, Message: "tool dispatch failed"}}); err != nil {
						p.fail(err)
					}
					return
				}
				b, err := json.Marshal(result)
				if err != nil {
					p.fail(err)
					return
				}
				if err := p.write(RunnerFrame{Attachment: f.Attachment, Turn: f.Turn, ID: f.ID, Result: b}); err != nil {
					p.fail(err)
				}
			}(f, call)
		case f.ID != "" && f.Method == "" && (len(f.Result) > 0 || f.Error != nil):
			p.mu.Lock()
			ch := p.pending[f.ID]
			delete(p.pending, f.ID)
			p.mu.Unlock()
			if ch == nil {
				p.fail(errors.New("unexpected harness response"))
				return
			}
			ch <- f
		default:
			p.fail(errors.New("unknown harness protocol RunnerFrame"))
			return
		}
	}
	err := s.Err()
	if err == nil {
		err = io.ErrUnexpectedEOF
	}
	p.fail(err)
}

func (p *process) write(f RunnerFrame) error {
	b, err := json.Marshal(f)
	if err != nil {
		return err
	}
	if len(b)+1 > MaxFrame {
		return errors.New("harness RunnerFrame too large")
	}
	p.writeMu.Lock()
	defer p.writeMu.Unlock()
	_, err = p.in.Write(append(b, '\n'))
	return err
}

func (a *attachment) request(ctx context.Context, method string, params any) (json.RawMessage, error) {
	p := a.host
	a.mu.Lock()
	turn := a.turn
	a.mu.Unlock()
	if method == "open" || method == "checkpoint" || method == "detach" {
		turn = ""
	}
	b, err := json.Marshal(params)
	if err != nil {
		return nil, err
	}
	id := core.ID()
	ch := make(chan RunnerFrame, 1)
	p.mu.Lock()
	if p.err != nil {
		err = p.err
		p.mu.Unlock()
		return nil, err
	}
	p.pending[id] = ch
	p.mu.Unlock()
	if err := p.write(RunnerFrame{Attachment: a.id, Turn: turn, ID: id, Method: method, Params: b}); err != nil {
		p.fail(err)
		return nil, err
	}
	select {
	case f := <-ch:
		if f.Attachment != a.id || f.Turn != turn {
			p.fail(errors.New("misattributed harness response"))
			return nil, errors.New("misattributed harness response")
		}
		if f.Error != nil {
			return nil, f.Error
		}
		return f.Result, nil
	case <-ctx.Done():
		if method == "prompt" {
			stopCtx, stop := context.WithTimeout(context.Background(), 5*time.Second)
			defer stop()
			if _, err := a.request(stopCtx, "interrupt", nil); err == nil {
				select {
				case <-ch:
					return nil, ctx.Err()
				case <-stopCtx.Done():
				case <-p.done:
				}
			}
		}
		p.fail(ctx.Err())
		return nil, ctx.Err()
	case <-p.done:
		p.mu.Lock()
		err := p.err
		p.mu.Unlock()
		return nil, fmt.Errorf("harness stopped: %w", err)
	}
}

type attachment struct {
	id       string
	host     *process
	factory  *ProcessFactory
	opts     Options
	mu       sync.Mutex
	turn     string
	calls    sync.WaitGroup
	once     sync.Once
	closeErr error
}

func (f *ProcessFactory) Open(ctx context.Context, opts Options) (Runner, error) {
	f.mu.Lock()
	if f.starting == nil {
		if f.MaxShared == 0 {
			f.MaxShared = 1
		}
		if f.MaxActiveTurns == 0 {
			f.MaxActiveTurns = f.MaxShared
		}
		if f.MaxHosts == 0 {
			f.MaxHosts = 64
		}
		if f.MaxStarting == 0 {
			f.MaxStarting = 4
		}
		if f.MaxShared < 1 || f.MaxShared > 1024 || f.MaxActiveTurns < 1 || f.MaxActiveTurns > f.MaxShared || f.MaxHosts < 1 || f.MaxStarting < 1 {
			f.mu.Unlock()
			return nil, errors.New("invalid harness host limits")
		}
		f.starting = make(chan struct{}, f.MaxStarting)
	}
	f.mu.Unlock()
	select {
	case f.starting <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	defer func() { <-f.starting }()
	f.mu.Lock()
	var host *process
	alive := f.pool[:0]
	for _, p := range f.pool {
		select {
		case <-p.done:
			continue
		default:
		}
		alive = append(alive, p)
		p.mu.Lock()
		available := len(p.attachments) < f.MaxShared
		p.mu.Unlock()
		if host == nil && available {
			host = p
		}
	}
	f.pool = alive
	if host == nil {
		if len(f.pool) >= f.MaxHosts {
			f.mu.Unlock()
			return nil, core.E(core.Overloaded, "harness host capacity reached")
		}
		var err error
		host, err = f.start(ctx, opts)
		if err != nil {
			f.mu.Unlock()
			return nil, err
		}
		f.pool = append(f.pool, host)
	}
	a := &attachment{id: core.ID(), host: host, factory: f, opts: opts}
	host.mu.Lock()
	host.attachments[a.id] = a
	host.mu.Unlock()
	f.mu.Unlock()
	initCtx, stop := context.WithTimeout(ctx, 30*time.Second)
	defer stop()
	if _, err := a.request(initCtx, "open", opts); err != nil {
		a.Close()
		return nil, err
	}
	return a, nil
}
func (a *attachment) Prompt(ctx context.Context, text string) error {
	select {
	case a.host.active <- struct{}{}:
	case <-ctx.Done():
		return ctx.Err()
	case <-a.host.done:
		return errors.New("harness host stopped")
	}
	defer func() { <-a.host.active }()
	a.mu.Lock()
	if a.turn != "" {
		a.mu.Unlock()
		return errors.New("session is already active")
	}
	a.turn = core.ID()
	a.mu.Unlock()
	defer func() { a.mu.Lock(); a.turn = ""; a.mu.Unlock() }()
	_, err := a.request(ctx, "prompt", core.Text{Text: text})
	return err
}
func (a *attachment) Steer(ctx context.Context, text string) error {
	_, err := a.request(ctx, "steer", core.Text{Text: text})
	return err
}
func (a *attachment) Interrupt(ctx context.Context) error {
	_, err := a.request(ctx, "interrupt", nil)
	return err
}
func (a *attachment) Checkpoint(ctx context.Context) ([]core.NativeFile, error) {
	b, err := a.request(ctx, "checkpoint", nil)
	if err != nil {
		return nil, err
	}
	var files []core.NativeFile
	err = json.Unmarshal(b, &files)
	return files, err
}
func (a *attachment) Close() error {
	a.once.Do(func() {
		a.calls.Wait()
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		select {
		case <-a.host.done:
		default:
			_, a.closeErr = a.request(ctx, "detach", nil)
		}
		f := a.factory
		f.mu.Lock()
		a.host.mu.Lock()
		delete(a.host.attachments, a.id)
		empty := len(a.host.attachments) == 0
		a.host.mu.Unlock()
		if empty {
			a.host.fail(errors.New("harness host empty"))
		}
		f.mu.Unlock()
		if empty {
			a.host.Close()
		}
	})
	return a.closeErr
}
func (p *process) Close() error {
	p.fail(errors.New("harness closed"))
	p.in.Close()
	<-p.waited
	<-p.readDone
	p.callWG.Wait()
	return os.RemoveAll(p.directory)
}
