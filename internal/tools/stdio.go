package tools

import (
	"context"
	"github.com/aexhq/brain/internal/harness"
	"github.com/modelcontextprotocol/go-sdk/mcp"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

// StdioConfig is installed application code, never session-request configuration.
// Each discovery/invocation owns an isolated process tree and private directory.
type StdioConfig struct {
	Command []string
	Env     map[string]string
}

func connectStdio(ctx context.Context, client *mcp.Client, b Binding) (*mcp.ClientSession, func(), error) {
	dir, err := os.MkdirTemp("", "brain-mcp-")
	if err != nil {
		return nil, nil, err
	}
	cleanup := func() { _ = os.RemoveAll(dir) }
	args := append([]string{}, b.Stdio.Command...)
	for i := 1; i < len(args); i++ {
		if info, e := os.Stat(args[i]); e == nil && info.Mode().IsRegular() {
			args[i], err = filepath.Abs(args[i])
			if err != nil {
				cleanup()
				return nil, nil, err
			}
		}
	}
	cmd := exec.CommandContext(ctx, args[0], args[1:]...)
	cmd.Dir = dir
	cmd.WaitDelay = 3 * time.Second
	for _, key := range []string{"PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "TMP", "TEMP", "TMPDIR", "LANG", "LC_ALL"} {
		if value, ok := os.LookupEnv(key); ok {
			cmd.Env = append(cmd.Env, key+"="+value)
		}
	}
	cmd.Env = append(cmd.Env, "HOME="+dir, "USERPROFILE="+dir)
	for key, value := range b.Stdio.Env {
		cmd.Env = append(cmd.Env, key+"="+value)
	}
	cmd.Stderr = io.Discard
	in, err := cmd.StdinPipe()
	if err != nil {
		cleanup()
		return nil, nil, err
	}
	out, err := cmd.StdoutPipe()
	if err != nil {
		_ = in.Close()
		cleanup()
		return nil, nil, err
	}
	kill, err := harness.StartOwned(cmd)
	if err != nil {
		_ = in.Close()
		_ = out.Close()
		cleanup()
		return nil, nil, err
	}
	done := make(chan struct{})
	go func() { _ = cmd.Wait(); close(done) }()
	stop := func() { _ = kill(); _ = in.Close(); <-done; cleanup() }
	session, err := client.Connect(ctx, &mcp.IOTransport{Reader: out, Writer: in, MaxLineLength: int(b.MaxOutputBytes + 65536)}, nil)
	if err != nil {
		stop()
		return nil, nil, err
	}
	return session, func() { _ = session.Close(); stop() }, nil
}
