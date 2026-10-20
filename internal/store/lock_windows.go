package store

import (
	"fmt"
	"golang.org/x/sys/windows"
	"os"
	"path/filepath"
)

func lockDirectory(root string) (*os.File, error) {
	f, err := os.OpenFile(filepath.Join(root, "owner.lock"), os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		return nil, err
	}
	// Lock beyond EOF so private-state inspection can still read the empty file.
	if err := windows.LockFileEx(windows.Handle(f.Fd()), windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &windows.Overlapped{OffsetHigh: 1}); err != nil {
		f.Close()
		return nil, fmt.Errorf("data directory is already owned: %w", err)
	}
	return f, nil
}
