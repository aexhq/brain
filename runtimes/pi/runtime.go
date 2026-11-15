// Package pi installs Pi's protocol over an isolated Sandbox environment.
package pi

import (
	"context"
	"errors"
	"io"
	"sync"
	"time"

	"github.com/aexhq/brain/runtimes/host"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/sandbox/api"
)

type State interface {
	ReadNative(context.Context, string, io.Writer) error
	CaptureNative(context.Context, string, []core.NativeFile, io.Reader) error
}

func Factory(client *api.Client, state State, maxBytes int64) *harness.ProcessFactory {
	return &harness.ProcessFactory{Steering: true, MaxShared: 1, MaxActiveTurns: 1, Launch: func(ctx context.Context, o harness.Options) (harness.Connection, error) {
		env, err := client.Create(ctx, api.Create{Tenant: core.Tenant(ctx), Session: o.SessionID, Profile: "pi"})
		if err != nil {
			return nil, err
		}
		c := &connection{client: client, id: env.ID, session: o.SessionID, state: state, maxBytes: maxBytes}
		ok := false
		defer func() {
			if !ok {
				_ = c.Close()
			}
		}()
		if err = importNative(ctx, c, o.SessionID); err != nil {
			return nil, err
		}
		lifetime := o.Lifetime
		if lifetime == nil {
			lifetime = ctx
		}
		c.ReadWriteCloser, err = client.Process(lifetime, env.ID, api.Process{Command: []string{"node", "/app/runtimes/pi/dist/main.js"}})
		if err != nil {
			return nil, err
		}
		ok = true
		return c, nil
	}}
}

type connection struct {
	io.ReadWriteCloser
	client      *api.Client
	id, session string
	state       State
	maxBytes    int64
	once        sync.Once
	err         error
}

func (c *connection) Directory() string { return "/workspace/native" }
func (c *connection) Close() error {
	c.once.Do(func() {
		if c.ReadWriteCloser != nil {
			_ = c.ReadWriteCloser.Close()
		}
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		c.err = c.client.Remove(ctx, c.id)
	})
	return c.err
}
func importNative(ctx context.Context, c *connection, id string) error {
	reader, writer := io.Pipe()
	done := make(chan error, 1)
	go func() { err := c.state.ReadNative(ctx, id, writer); _ = writer.CloseWithError(err); done <- err }()
	err := c.client.Import(ctx, c.id, reader)
	_ = reader.CloseWithError(err)
	return errors.Join(err, <-done)
}
func (c *connection) Export(ctx context.Context, files []core.NativeFile) error {
	reader, writer := io.Pipe()
	done := make(chan error, 1)
	go func() {
		err := c.client.Export(ctx, c.id, writer, c.maxBytes)
		_ = writer.CloseWithError(err)
		done <- err
	}()
	err := c.state.CaptureNative(ctx, c.session, files, reader)
	_ = reader.CloseWithError(err)
	return errors.Join(err, <-done)
}
