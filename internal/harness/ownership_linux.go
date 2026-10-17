package harness

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"syscall"
	"time"
)

const guardianArgument = "--brain-owned-harness"

// The guardian survives long enough to kill the entire process group when its
// owner dies. Pdeathsig on the native root alone would leave grandchildren alive.
func init() {
	if len(os.Args) < 3 || os.Args[1] != guardianArgument || os.Getenv("BRAIN_HOST_GUARDIAN") != "1" {
		return
	}
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, syscall.SIGTERM, syscall.SIGINT)
	// The owning process needs the native group ID even if this guardian is
	// itself killed. Descriptor 3 is a private startup pipe, never native stdio.
	group := os.NewFile(3, "native-group")
	syscall.CloseOnExec(3)
	cmd := exec.Command(os.Args[2], os.Args[3:]...)
	_ = os.Unsetenv("BRAIN_HOST_GUARDIAN")
	cmd.Env = os.Environ()
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	in, err := cmd.StdinPipe()
	if err != nil {
		os.Exit(1)
	}
	if err := cmd.Start(); err != nil {
		os.Exit(1)
	}
	if _, err := fmt.Fprintln(group, cmd.Process.Pid); err != nil {
		_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
		os.Exit(1)
	}
	_ = group.Close()
	inputDone := make(chan struct{})
	go func() { _, _ = io.Copy(in, os.Stdin); in.Close(); close(inputDone) }()
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	exited := false
	code := 1
	select {
	case err := <-done:
		exited = true
		if err == nil {
			code = 0
		}
	case <-inputDone:
		code = 0
	case <-signals:
	}
	_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
	if !exited {
		<-done
	}
	os.Exit(code)
}

func startOwned(cmd *exec.Cmd) (func() error, error) {
	executable, err := os.Executable()
	if err != nil {
		return nil, err
	}
	cmd.Args = append([]string{executable, guardianArgument, cmd.Path}, cmd.Args[1:]...)
	cmd.Path = executable
	cmd.Env = append(cmd.Env, "BRAIN_HOST_GUARDIAN=1")
	cmd.SysProcAttr = &syscall.SysProcAttr{Pdeathsig: syscall.SIGTERM}
	reader, writer, err := os.Pipe()
	if err != nil {
		return nil, err
	}
	defer reader.Close()
	cmd.ExtraFiles = []*os.File{writer}
	// CommandContext must signal the guardian, allowing it to reap descendants.
	cmd.Cancel = func() error { return cmd.Process.Signal(syscall.SIGTERM) }
	if err := cmd.Start(); err != nil {
		writer.Close()
		return nil, err
	}
	writer.Close()
	_ = reader.SetReadDeadline(time.Now().Add(10 * time.Second))
	var group int
	if _, err := fmt.Fscan(reader, &group); err != nil {
		_ = cmd.Process.Signal(syscall.SIGTERM)
		_ = cmd.Wait()
		return nil, err
	}
	return func() error {
		_ = syscall.Kill(-group, syscall.SIGKILL)
		return cmd.Process.Signal(syscall.SIGTERM)
	}, nil
}
