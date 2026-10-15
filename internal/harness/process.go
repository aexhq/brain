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
	"strings"
	"sync"
	"time"

	"github.com/aexhq/brain/internal/core"
)

const MaxFrame = 4 << 20

type ProcessFactory struct {
	Command  []string
	Steering bool
	Logger   *slog.Logger
}

func (f *ProcessFactory) SupportsSteering() bool { return f.Steering }

type RunnerFrame struct {
	ID     string          `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  *core.Error     `json:"error,omitempty"`
	Event  string          `json:"event,omitempty"`
	Data   json.RawMessage `json:"data,omitempty"`
}

type process struct {
	cmd      *exec.Cmd
	in       io.WriteCloser
	writeMu  sync.Mutex
	mu       sync.Mutex
	pending  map[string]chan RunnerFrame
	ctx      context.Context
	cancel   context.CancelFunc
	done     chan struct{}
	waited   chan struct{}
	readDone chan struct{}
	callWG   sync.WaitGroup
	err      error
	once     sync.Once
	opts     Options
	tools    chan struct{}
}

func (f *ProcessFactory) Open(ctx context.Context, opts Options) (Runner, error) {
	if len(f.Command) == 0 {
		return nil, core.E(core.Unsupported, "harness command is not configured")
	}
	procCtx, cancel := context.WithCancel(ctx)
	cmd := exec.CommandContext(procCtx, f.Command[0], f.Command[1:]...)
	// Inherit process necessities, not ambient credentials or harness settings.
	for _, key := range []string{"PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "TMP", "TEMP", "TMPDIR", "LANG", "LC_ALL"} {
		if value, ok := os.LookupEnv(key); ok {
			cmd.Env = append(cmd.Env, key+"="+value)
		}
	}
	cmd.Env = append(cmd.Env, "NO_COLOR=1")
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
	p := &process{cmd: cmd, in: in, pending: make(map[string]chan RunnerFrame), ctx: procCtx, cancel: cancel, done: make(chan struct{}), waited: make(chan struct{}), readDone: make(chan struct{}), opts: opts, tools: make(chan struct{}, 32)}
	if err := cmd.Start(); err != nil {
		cancel()
		return nil, err
	}
	go func() {
		s := bufio.NewScanner(stderr)
		s.Buffer(make([]byte, 4096), 64<<10)
		for s.Scan() {
			if f.Logger != nil {
				f.Logger.Debug("harness diagnostic", "session", opts.SessionID, "message", s.Text())
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
	initCtx, stop := context.WithTimeout(ctx, 30*time.Second)
	defer stop()
	if _, err := p.request(initCtx, "open", opts); err != nil {
		p.Close()
		return nil, err
	}
	return p, nil
}

func (p *process) fail(err error) {
	p.once.Do(func() { p.mu.Lock(); p.err = err; p.mu.Unlock(); close(p.done); p.cancel() })
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
		switch {
		case f.Event != "":
			if f.ID != "" || len(f.Data) == 0 {
				p.fail(errors.New("invalid harness event"))
				return
			}
			if err := p.opts.Event(f.Event, f.Data); err != nil {
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
			go func(f RunnerFrame, call core.Call) {
				defer p.callWG.Done()
				defer func() { <-p.tools }()
				result, err := p.opts.Call(p.ctx, call)
				if err != nil {
					p.fail(err)
					return
				}
				b, err := json.Marshal(result)
				if err != nil {
					p.fail(err)
					return
				}
				if err := p.write(RunnerFrame{ID: f.ID, Result: b}); err != nil {
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

func (p *process) request(ctx context.Context, method string, params any) (json.RawMessage, error) {
	stop := context.AfterFunc(ctx, func() { p.fail(ctx.Err()) })
	defer stop()
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
	if err := p.write(RunnerFrame{ID: id, Method: method, Params: b}); err != nil {
		p.fail(err)
		return nil, err
	}
	select {
	case f := <-ch:
		if f.Error != nil {
			return nil, f.Error
		}
		return f.Result, nil
	case <-ctx.Done():
		// A cancelled request may already have changed native state. Kill this
		// runner rather than accepting an uncorrelated late response.
		p.fail(ctx.Err())
		return nil, ctx.Err()
	case <-p.done:
		p.mu.Lock()
		err := p.err
		p.mu.Unlock()
		return nil, fmt.Errorf("harness stopped: %w", err)
	}
}

func (p *process) Prompt(ctx context.Context, text string) error {
	_, err := p.request(ctx, "prompt", core.Text{Text: text})
	return err
}
func (p *process) Steer(ctx context.Context, text string) error {
	_, err := p.request(ctx, "steer", core.Text{Text: text})
	return err
}
func (p *process) Interrupt(ctx context.Context) error {
	_, err := p.request(ctx, "interrupt", nil)
	return err
}
func (p *process) Checkpoint(ctx context.Context) ([]core.NativeFile, error) {
	b, err := p.request(ctx, "checkpoint", nil)
	if err != nil {
		return nil, err
	}
	var files []core.NativeFile
	if err := json.Unmarshal(b, &files); err != nil {
		return nil, err
	}
	return files, nil
}
func (p *process) Close() error {
	p.fail(errors.New("harness closed"))
	p.in.Close()
	<-p.waited
	<-p.readDone
	p.callWG.Wait()
	return nil
}
