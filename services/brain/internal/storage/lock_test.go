package store

import "testing"

func TestDirectoryHasOneOwnerAndReleasesAfterClose(t *testing.T) {
	root := t.TempDir()
	first, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	if second, err := Open(root); err == nil {
		second.Close()
		t.Fatal("second writer admitted")
	}
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	next, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	if err := next.Close(); err != nil {
		t.Fatal(err)
	}
}
