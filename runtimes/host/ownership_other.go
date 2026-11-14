//go:build !windows && !linux

package harness

import (
	"errors"
	"os/exec"
)

func startOwned(*exec.Cmd) (func() error, error) {
	return nil, errors.New("owned harness processes require Linux or Windows")
}
