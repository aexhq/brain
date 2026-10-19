package harness

import (
	"context"
	"encoding/json"
	"os"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
)

func TestCommonPoolMixedFactoriesBudgetsAndDrain(t *testing.T) {
	for _, limits := range []PoolLimits{{}, {Hosts: 2, ActiveTurns: 1}} {
		t.Run(map[bool]string{true: "limited", false: "unlimited"}[limits.Hosts != 0], func(t *testing.T) {
			pool, err := NewHostPool(limits)
			if err != nil {
				t.Fatal(err)
			}
			defer pool.Close()
			exe, err := os.Executable()
			if err != nil {
				t.Fatal(err)
			}
			factory := func() *ProcessFactory {
				return &ProcessFactory{Pool: pool, Command: []string{exe, "-test.run=^TestSharedHostHelper$", "brain-test-host"}, MaxShared: 1, MaxHosts: 3}
			}
			first, second := factory(), factory()
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			started := make(chan struct{}, 4)
			opts := Options{Lifetime: ctx, SessionID: core.ID(), Event: func(string, json.RawMessage) error { started <- struct{}{}; return nil }}
			a, err := first.Open(ctx, opts)
			if err != nil {
				t.Fatal(err)
			}
			b, err := second.Open(ctx, opts)
			if err != nil {
				t.Fatal(err)
			}
			extra, err := second.Open(ctx, opts)
			if limits.Hosts > 0 {
				if core.ErrorCode(err) != core.Overloaded {
					t.Fatalf("aggregate host cap: %v", err)
				}
			} else {
				if err != nil {
					t.Fatalf("omitted limits restricted hosts: %v", err)
				}
				if err := extra.Close(); err != nil {
					t.Fatal(err)
				}
			}
			finished := make(chan error, 2)
			go func() { finished <- a.Prompt(ctx, "wait") }()
			select {
			case <-started:
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
			if limits.ActiveTurns > 0 {
				if err := b.Prompt(ctx, "excess"); core.ErrorCode(err) != core.Overloaded {
					t.Fatalf("aggregate turn cap: %v", err)
				}
			} else {
				go func() { finished <- b.Prompt(ctx, "wait") }()
				select {
				case <-started:
				case <-ctx.Done():
					t.Fatal(ctx.Err())
				}
			}
			if err := a.Interrupt(ctx); err != nil {
				t.Fatal(err)
			}
			if err := <-finished; err != nil {
				t.Fatal(err)
			}
			if limits.ActiveTurns > 0 {
				go func() { finished <- b.Prompt(ctx, "released") }()
				select {
				case <-started:
				case <-ctx.Done():
					t.Fatal(ctx.Err())
				}
			}
			dir := b.(*attachment).host.directory
			if err := pool.Close(); err != nil {
				t.Fatal(err)
			}
			if err := <-finished; err == nil {
				t.Fatal("pool drain reported active inference as success")
			}
			if _, err := os.Stat(dir); !os.IsNotExist(err) {
				t.Fatalf("owned host directory remains: %v", err)
			}
			if _, err := first.Open(ctx, opts); core.ErrorCode(err) != core.Overloaded {
				t.Fatalf("drained pool reopened: %v", err)
			}
			_ = a.Close()
			_ = b.Close()
		})
	}
}

func TestCommonPoolStartupBudgetIsReleasedAfterFailedOpen(t *testing.T) {
	pool, err := NewHostPool(PoolLimits{Starting: 1})
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	factory := func() *ProcessFactory {
		return &ProcessFactory{Pool: pool, Command: []string{exe, "-test.run=^TestSharedHostHelper$", "brain-test-host"}}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	dir := t.TempDir()
	type opened struct {
		runner Runner
		err    error
	}
	opening := make(chan opened, 1)
	go func() {
		r, e := factory().Open(ctx, Options{Lifetime: ctx, Dir: dir, Config: core.Config{Model: "wait-open"}})
		opening <- opened{r, e}
	}()
	for {
		if _, err := os.Stat(dir + "/started"); err == nil {
			break
		}
		select {
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		default:
			time.Sleep(time.Millisecond)
		}
	}
	if _, err := factory().Open(ctx, Options{Lifetime: ctx}); core.ErrorCode(err) != core.Overloaded {
		t.Fatalf("aggregate startup cap: %v", err)
	}
	if err := os.WriteFile(dir+"/release", nil, 0600); err != nil {
		t.Fatal(err)
	}
	first := <-opening
	if first.err != nil {
		t.Fatal(first.err)
	}
	if err := first.runner.Close(); err != nil {
		t.Fatal(err)
	}
	bad := Options{Lifetime: ctx, Config: core.Config{Model: "invalid"}}
	if _, err := factory().Open(ctx, bad); core.ErrorCode(err) != core.Invalid {
		t.Fatalf("failed open: %v", err)
	}
	good, err := factory().Open(ctx, Options{Lifetime: ctx})
	if err != nil {
		t.Fatalf("startup reservation leaked: %v", err)
	}
	if err := good.Close(); err != nil {
		t.Fatal(err)
	}
}
