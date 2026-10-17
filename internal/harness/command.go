package harness

import "os/exec"

// StartOwned starts a trusted installed subprocess in the same lifetime boundary
// as native harnesses. The caller closes the tree and waits for the root process.
func StartOwned(cmd *exec.Cmd) (func() error, error) { return startOwned(cmd) }
