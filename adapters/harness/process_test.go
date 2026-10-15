package harness

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"slices"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestDecisionWorkerFixture(t *testing.T) {
	if !slices.Contains(os.Args, "brain-decision-fixture") {
		return
	}
	reader := json.NewDecoder(os.Stdin)
	count := 0
	for {
		var request wire.HarnessRequest
		if reader.Decode(&request) != nil {
			os.Exit(0)
		}
		if kernel.Value(request.Input.Text) == "block" {
			time.Sleep(time.Minute)
		}
		count++
		_ = json.NewEncoder(os.Stdout).Encode(wire.Decision{Kind: "complete", Result: kernel.JSON(request.Input.Text), Checkpoint: kernel.JSON(map[string]any{"requests": count})})
		if kernel.Value(request.Input.Text) == "exit-after-response" {
			os.Exit(0)
		}
	}
}

func TestContinuationRequiresItsOriginalProcessUnlessDeclaredRepeatable(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, repeatable := range []bool{false, true} {
		t.Run(map[bool]string{false: "process_bound", true: "checkpointed"}[repeatable], func(t *testing.T) {
			registry, err := New([]Registration{{Name: "fixture", Version: "1", Command: executable,
				Arguments: []string{"-test.run=TestDecisionWorkerFixture", "--", "brain-decision-fixture"}, Workers: 1,
				Models: []string{"fixture"}, Continuation: repeatable}})
			if err != nil {
				t.Fatal(err)
			}
			defer registry.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			request := wire.HarnessRequest{Protocol: "brain-decision/1", SessionID: "session", RunID: "run",
				Config: wire.SessionConfig{Harness: wire.HarnessSelection{Name: "fixture", Version: "1"}},
				Input:  wire.InputData{Kind: "message", Text: kernel.Ptr("one response")}}
			admissions := 0
			admit := func(process string, capability bool) error {
				admissions++
				if !kernel.Identifier(process) || capability != repeatable {
					t.Fatal("incorrect process boundary")
				}
				return nil
			}
			if _, _, err = registry.Decide(ctx, request, "", func(string, bool) error {
				return kernel.Fail(wire.ErrorCodeConflict, "durable admission rejected")
			}); kernel.Response(err).Code != wire.ErrorCodeConflict {
				t.Fatal(err)
			}
			decision, process, err := registry.Decide(ctx, request, "", admit)
			if err != nil || admissions != 1 {
				t.Fatalf("first decision failed: %v", err)
			}
			if string(decision.Checkpoint) != `{"requests":1}` {
				t.Fatal("denied admission reached child stdin")
			}
			decision, same, err := registry.Decide(ctx, request, process, admit)
			if err != nil || process != same || string(decision.Checkpoint) != `{"requests":2}` {
				t.Fatalf("live process identity was not preserved: %s %v", decision.Checkpoint, err)
			}
			w := registry.workers["fixture"][0]
			if err = w.command.Process.Kill(); err != nil {
				t.Fatal(err)
			}
			<-w.exited
			err = registry.Resume(ctx, request.Config, request.RunID, process)
			if !repeatable && kernel.Response(err).Code != wire.ErrorCodeUnsupported {
				t.Fatalf("lost process was silently resumed: %v", err)
			}
			fresh := request
			fresh.RunID = "new-explicit-run"
			_, replacement, err := registry.Decide(ctx, fresh, "", admit)
			if err != nil || replacement == process {
				t.Fatalf("explicit fresh work did not obtain a new process identity: %s %v", replacement, err)
			}
			_, _, err = registry.Decide(ctx, request, process, admit)
			if repeatable && err != nil {
				t.Fatal(err)
			}
			if !repeatable && (kernel.Response(err).Code != wire.ErrorCodeUnsupported || admissions != 3) {
				t.Fatalf("replacement process accepted old nonrepeatable work: %d %v", admissions, err)
			}
		})
	}
}

func TestFinalResponseSurvivesExitAndCancellationReleasesItsLane(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	registry, err := New([]Registration{{Name: "fixture", Version: "1", Command: executable,
		Arguments: []string{"-test.run=TestDecisionWorkerFixture", "--", "brain-decision-fixture"}, Workers: 1,
		Models: []string{"fixture"}, Continuation: true}})
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	request := wire.HarnessRequest{Protocol: "brain-decision/1", SessionID: "session", RunID: "run",
		Config: wire.SessionConfig{Harness: wire.HarnessSelection{Name: "fixture", Version: "1"}},
		Input:  wire.InputData{Kind: "message", Text: kernel.Ptr("exit-after-response")}}
	admit := func(string, bool) error { return nil }
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	decision, process, err := registry.Decide(ctx, request, "", admit)
	if err != nil || string(decision.Result) != `"exit-after-response"` {
		t.Fatalf("process reaper lost its final response: %+v %v", decision, err)
	}
	<-registry.workers["fixture"][0].exited
	blocked := request
	blocked.Input.Text = kernel.Ptr("block")
	active, stop := context.WithCancel(ctx)
	defer stop()
	started := make(chan struct{})
	done := make(chan error, 1)
	go func() {
		_, _, err := registry.Decide(active, blocked, process, func(string, bool) error { close(started); return nil })
		done <- err
	}()
	select {
	case <-started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	waiting, release := context.WithTimeout(ctx, 30*time.Millisecond)
	defer release()
	if _, _, err = registry.Decide(waiting, request, process, func(string, bool) error {
		t.Fatal("a second invocation bypassed an occupied lane")
		return nil
	}); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("lane acquisition ignored cancellation: %v", err)
	}
	stop()
	if err = <-done; !errors.Is(err, context.Canceled) {
		t.Fatalf("active invocation did not stop: %v", err)
	}
	request.Input.Text = kernel.Ptr("after cancellation")
	if _, _, err = registry.Decide(ctx, request, process, admit); err != nil {
		t.Fatal(err)
	}
}
