package store

import "testing"

func TestDirectoryHasOneOwnerAndReleasesAfterClose(t *testing.T) {
	root := t.TempDir()
	first, err := Open(root, nil)
	if err != nil {
		t.Fatal(err)
	}
	if second, err := Open(root, nil); err == nil {
		second.Close()
		t.Fatal("second writer admitted")
	}
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	next, err := Open(root, nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := next.Close(); err != nil {
		t.Fatal(err)
	}
}
