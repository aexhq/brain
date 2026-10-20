package harness

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"github.com/aexhq/brain/internal/core"
	"os"
	"sync"
	"testing"
	"time"
)

func TestSharedHostHelper(t *testing.T) {
	if len(os.Args) < 3 || os.Args[len(os.Args)-1] != "brain-test-host" {
		return
	}
	var mu sync.Mutex
	turns := map[string]chan struct{}{}
	send := func(f RunnerFrame) { mu.Lock(); defer mu.Unlock(); _ = json.NewEncoder(os.Stdout).Encode(f) }
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		var frame RunnerFrame
		if json.Unmarshal(scanner.Bytes(), &frame) != nil {
			os.Exit(2)
		}
		switch frame.Method {
		case "open", "detach":
			if frame.Method == "open" {
				var options Options
				if json.Unmarshal(frame.Params, &options) != nil {
					os.Exit(4)
				}
				if options.Config.Model == "invalid" {
					frame.Method = ""
					frame.Params = nil
					frame.Error = &core.Error{Code: core.Invalid, Message: "invalid model"}
					send(frame)
					continue
				}
				if options.Config.Model == "wait-open" {
					if os.WriteFile(options.Dir+"/started", []byte("ready"), 0600) != nil {
						os.Exit(5)
					}
					for {
						if _, err := os.Stat(options.Dir + "/release"); err == nil {
							break
						}
						time.Sleep(time.Millisecond)
					}
				}
			}
			frame.Method = ""
			frame.Params = nil
			frame.Result = json.RawMessage(`{}`)
			send(frame)
		case "prompt":
			done := make(chan struct{})
			mu.Lock()
			turns[frame.Attachment] = done
			mu.Unlock()
			send(RunnerFrame{Attachment: frame.Attachment, Turn: frame.Turn, Event: "started", Data: json.RawMessage(`{}`)})
			go func(f RunnerFrame) { <-done; f.Method = ""; f.Params = nil; f.Result = json.RawMessage(`{}`); send(f) }(frame)
		case "interrupt":
			mu.Lock()
			done := turns[frame.Attachment]
			delete(turns, frame.Attachment)
			mu.Unlock()
			if done != nil {
				close(done)
			}
			frame.Method = ""
			frame.Params = nil
			frame.Result = json.RawMessage(`{}`)
			send(frame)
		default:
			os.Exit(3)
		}
	}
	os.Exit(0)
}

func TestSharedHostAdmissionActiveCapsCancellationAndCrash(t *testing.T) {
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	f := &ProcessFactory{Command: []string{exe, "-test.run=^TestSharedHostHelper$", "brain-test-host"}, MaxShared: 8, MaxActiveTurns: 2, MaxHosts: 1, MaxStarting: 4}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	started := make(chan string, 16)
	options := func(id string) Options {
		return Options{Lifetime: ctx, SessionID: id, Event: func(_ string, _ json.RawMessage) error { started <- id; return nil }, Call: func(context.Context, core.Call) (core.Result, error) { return core.Result{}, nil }}
	}
	type opened struct {
		runner Runner
		err    error
	}
	opens := make(chan opened, 9)
	for i := 0; i < 9; i++ {
		go func() { r, e := f.Open(ctx, options(core.ID())); opens <- opened{r, e} }()
	}
	runners := []Runner{}
	for i := 0; i < 9; i++ {
		result := <-opens
		if result.err == nil {
			runners = append(runners, result.runner)
			defer result.runner.Close()
		} else if core.ErrorCode(result.err) != core.Overloaded {
			t.Fatal(result.err)
		}
	}
	if len(runners) != 8 || len(f.group.hosts) != 1 {
		t.Fatalf("admission: %d sessions in %d hosts", len(runners), len(f.group.hosts))
	}
	finished := make(chan error, 2)
	for _, r := range runners[:2] {
		go func() { finished <- r.Prompt(ctx, core.Text{Text: "wait"}) }()
	}
	for i := 0; i < 2; i++ {
		select {
		case <-started:
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	queued, stop := context.WithTimeout(ctx, 100*time.Millisecond)
	if err := runners[2].Prompt(queued, core.Text{Text: "queued"}); err == nil {
		t.Fatal("active cap ignored")
	}
	stop()
	select {
	case <-started:
		t.Fatal("queued turn started over cap")
	default:
	}
	if err := runners[0].Interrupt(ctx); err != nil {
		t.Fatal(err)
	}
	if err := <-finished; err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-finished:
		t.Fatalf("peer was cancelled: %v", err)
	default:
	}
	if err := runners[0].Close(); err != nil {
		t.Fatal(err)
	}
	bad := options("bad")
	bad.Config.Model = "invalid"
	if _, err := f.Open(ctx, bad); core.ErrorCode(err) != core.Invalid {
		t.Fatalf("bad attachment: %v", err)
	}
	select {
	case err := <-finished:
		t.Fatalf("failed open stopped a peer: %v", err)
	default:
	}
	replacement, err := f.Open(ctx, options("replacement"))
	if err != nil {
		t.Fatal(err)
	}
	defer replacement.Close()
	if len(f.group.hosts) != 1 {
		t.Fatal("idle eviction failed to reuse host")
	}
	go func() { finished <- replacement.Prompt(ctx, core.Text{Text: "wait"}) }()
	select {
	case <-started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	if err := f.group.hosts[0].cmd.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		select {
		case err := <-finished:
			if err == nil {
				t.Fatal("crash reported success")
			}
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	recovered, err := f.Open(ctx, options("recovered"))
	if err != nil {
		t.Fatal(err)
	}
	defer recovered.Close()
	select {
	case id := <-started:
		t.Fatal(fmt.Sprintf("reopen started inference for %s", id))
	default:
	}
}
