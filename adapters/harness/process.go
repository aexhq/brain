// Package harness runs installed implementations outside the API process using
// bounded, one-request-at-a-time JSON-lines workers.
package harness

import (
	"bufio"
	"context"
	"crypto/rand"
	"encoding/json"
	"hash/fnv"
	"io"
	"os"
	"os/exec"
	"slices"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/validation"
)

type Registration struct {
	Name         string   `json:"name"`
	Version      string   `json:"version"`
	Command      string   `json:"command"`
	Arguments    []string `json:"arguments"`
	Workers      int      `json:"workers"`
	Models       []string `json:"models"`
	Continuation bool     `json:"continuation"`
}

type Registry struct {
	registrations map[string]Registration
	workers       map[string][]*worker
	validator     *validation.Validator
}

type worker struct {
	gate         chan struct{}
	command      *exec.Cmd
	input        io.WriteCloser
	output       *bufio.Scanner
	reader       io.ReadCloser
	exited       chan struct{}
	exitError    error
	identity     string
	registration Registration
}

func New(registrations []Registration) (*Registry, error) {
	v, err := validation.New()
	if err != nil {
		return nil, err
	}
	r := &Registry{registrations: map[string]Registration{}, workers: map[string][]*worker{}, validator: v}
	for _, registration := range registrations {
		if !kernel.Identifier(registration.Name) || registration.Version == "" || registration.Workers < 1 || registration.Workers > 256 || registration.Command == "" {
			return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid installed harness registration")
		}
		if _, exists := r.registrations[registration.Name]; exists {
			return nil, kernel.Fail(wire.ErrorCodeInvalid, "duplicate harness name")
		}
		for range registration.Workers {
			w := &worker{registration: registration, gate: make(chan struct{}, 1)}
			w.gate <- struct{}{}
			r.workers[registration.Name] = append(r.workers[registration.Name], w)
		}
		r.registrations[registration.Name] = registration
	}
	return r, nil
}

func (r *Registry) Validate(config wire.SessionConfig) error {
	registration, ok := r.registrations[config.Harness.Name]
	if !ok || registration.Version != config.Harness.Version {
		return kernel.Fail(wire.ErrorCodeUnsupported, "selected harness version is not installed")
	}
	if !slices.Contains(registration.Models, config.Model.Provider) {
		return kernel.Fail(wire.ErrorCodeUnsupported, "harness does not support the selected model provider")
	}
	return nil
}

func (r *Registry) SupportsContinuation(name string) bool { return r.registrations[name].Continuation }

func (r *Registry) Capabilities() []string {
	out := []string{}
	for _, registration := range r.registrations {
		for _, model := range registration.Models {
			out = append(out, kernel.HarnessCapability(wire.HarnessSelection{Name: registration.Name, Version: registration.Version}, model))
			if registration.Continuation {
				out = append(out, kernel.Capability("continuation", registration.Name, registration.Version, model))
			}
		}
	}
	return out
}

func (r *Registry) lane(name, run string) (*worker, error) {
	values := r.workers[name]
	if len(values) == 0 {
		return nil, kernel.Fail(wire.ErrorCodeUnsupported, "harness is not installed")
	}
	hash := fnv.New32a()
	_, _ = hash.Write([]byte(run))
	return values[uint64(hash.Sum32())%uint64(len(values))], nil
}

func (w *worker) live() bool {
	if w.command == nil {
		return false
	}
	select {
	case <-w.exited:
		return false
	default:
		return true
	}
}

func (w *worker) resume(process string) error {
	if process != "" && !w.registration.Continuation && (!w.live() || w.identity != process) {
		return kernel.Fail(wire.ErrorCodeUnsupported, "harness does not support continuation after its original process was lost; cancel or start explicit new work")
	}
	return nil
}

func (r *Registry) Resume(ctx context.Context, config wire.SessionConfig, run, process string) error {
	w, err := r.lane(config.Harness.Name, run)
	if err != nil {
		return err
	}
	if process == "" || w.registration.Continuation {
		return nil
	}
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-w.gate:
	}
	defer func() { w.gate <- struct{}{} }()
	return w.resume(process)
}

func (w *worker) start() error {
	cmd := exec.Command(w.registration.Command, w.registration.Arguments...)
	for _, name := range []string{"PATH", "SystemRoot", "PATHEXT", "HOME", "USERPROFILE"} {
		if value, ok := os.LookupEnv(name); ok {
			cmd.Env = append(cmd.Env, name+"="+value)
		}
	}
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return err
	}
	// A separately owned reader keeps final response bytes readable after the
	// process reaper observes exit; Cmd.Wait must not close that reader.
	stdout, childOutput, err := os.Pipe()
	if err != nil {
		stdin.Close()
		return err
	}
	cmd.Stdout, cmd.Stderr = childOutput, io.Discard
	if err = cmd.Start(); err != nil {
		stdin.Close()
		stdout.Close()
		childOutput.Close()
		return err
	}
	_ = childOutput.Close()
	scanner := bufio.NewScanner(stdout)
	scanner.Buffer(make([]byte, 4096), 262144)
	w.command, w.input, w.output, w.reader = cmd, stdin, scanner, stdout
	w.identity = rand.Text()
	w.exited = make(chan struct{})
	go func() {
		w.exitError = cmd.Wait()
		close(w.exited)
	}()
	return nil
}

func (w *worker) stop() error {
	if w.command == nil {
		return nil
	}
	_ = w.input.Close()
	_ = w.command.Process.Kill()
	<-w.exited
	_ = w.reader.Close()
	err := w.exitError
	w.command, w.exited = nil, nil
	return err
}

func (r *Registry) Decide(ctx context.Context, request wire.HarnessRequest, process string, admit func(string, bool) error) (wire.Decision, string, error) {
	w, err := r.lane(request.Config.Harness.Name, request.RunID)
	if err != nil {
		return wire.Decision{}, "", err
	}
	select {
	case <-ctx.Done():
		return wire.Decision{}, "", ctx.Err()
	case <-w.gate:
	}
	defer func() { w.gate <- struct{}{} }()
	if err = w.resume(process); err != nil {
		return wire.Decision{}, "", err
	}
	if !w.live() {
		_ = w.stop()
		if err = w.start(); err != nil {
			return wire.Decision{}, "", err
		}
	}
	body := append(kernel.JSON(request), '\n')
	if len(body) > 1048576 {
		return wire.Decision{}, "", kernel.Fail(wire.ErrorCodeInvalid, "harness context exceeds one MiB")
	}
	if admit == nil {
		return wire.Decision{}, "", kernel.Fail(wire.ErrorCodeInvalid, "harness invocation requires its durable decision boundary")
	}
	if err = admit(w.identity, w.registration.Continuation); err != nil {
		return wire.Decision{}, "", err
	}
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	type result struct {
		body []byte
		err  error
	}
	done := make(chan result, 1)
	go func() {
		if _, err := w.input.Write(body); err != nil {
			done <- result{err: err}
			return
		}
		if !w.output.Scan() {
			err := w.output.Err()
			if err == nil {
				err = io.ErrUnexpectedEOF
			}
			done <- result{err: err}
			return
		}
		done <- result{body: append([]byte(nil), w.output.Bytes()...)}
	}()
	var response result
	select {
	case response = <-done:
	case <-ctx.Done():
		_ = w.stop()
		<-done
		return wire.Decision{}, "", ctx.Err()
	}
	if response.err != nil {
		_ = w.stop()
		return wire.Decision{}, "", response.err
	}
	if err = r.validator.Wire("Decision", response.body); err != nil {
		_ = w.stop()
		return wire.Decision{}, "", err
	}
	var decision wire.Decision
	err = json.Unmarshal(response.body, &decision)
	return decision, w.identity, err
}

func (r *Registry) Close() error {
	for _, values := range r.workers {
		for _, w := range values {
			<-w.gate
			_ = w.stop()
			w.gate <- struct{}{}
		}
	}
	return nil
}
