package harness

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

// A heartbeat proves that a real descendant stopped, including after the owner
// itself is killed without running cleanup code.
func TestOwnershipHelper(t *testing.T) {
	role := os.Getenv("BRAIN_OWNERSHIP_TEST")
	if role == "" {
		return
	}
	file := os.Getenv("BRAIN_OWNERSHIP_FILE")
	if role == "leaf" {
		for {
			if os.WriteFile(file, []byte(fmt.Sprint(time.Now().UnixNano())), 0600) != nil {
				os.Exit(2)
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	exe, _ := os.Executable()
	child := exec.CommandContext(context.Background(), exe, "-test.run=^TestOwnershipHelper$")
	child.Env = append(os.Environ(), "BRAIN_OWNERSHIP_TEST=leaf")
	if role == "owner" {
		child.Env = append(os.Environ(), "BRAIN_OWNERSHIP_TEST=branch")
	}
	if role == "owner" {
		// Keep the guardian's input open until abrupt owner death.
		in, err := child.StdinPipe()
		if err != nil {
			os.Exit(3)
		}
		defer in.Close()
		if _, err := startOwned(child); err != nil {
			os.Exit(4)
		}
	} else if err := child.Start(); err != nil {
		os.Exit(5)
	}
	for {
		time.Sleep(time.Hour)
	}
}

func TestOwnedTreeDiesWithParent(t *testing.T) {
	file := filepath.Join(t.TempDir(), "heartbeat")
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	owner := exec.Command(exe, "-test.run=^TestOwnershipHelper$")
	owner.Env = append(os.Environ(), "BRAIN_OWNERSHIP_TEST=owner", "BRAIN_OWNERSHIP_FILE="+file)
	if err := owner.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = owner.Process.Kill(); _ = owner.Wait() })
	deadline := time.Now().Add(10 * time.Second)
	for {
		if _, err := os.Stat(file); err == nil {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("descendant did not start")
		}
		time.Sleep(20 * time.Millisecond)
	}
	if err := owner.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	_ = owner.Wait()
	time.Sleep(300 * time.Millisecond)
	before, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	time.Sleep(250 * time.Millisecond)
	after, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	if string(before) != string(after) {
		t.Fatal("owned descendant survived abrupt parent death")
	}
}
