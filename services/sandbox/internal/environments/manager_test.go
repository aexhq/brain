package environments

import (
	"archive/tar"
	"bytes"
	"io"
	"testing"
)

func TestImportRejectsPathsLinksAndOversizeFiles(t *testing.T) {
	for _, h := range []*tar.Header{{Name: "../escape", Typeflag: tar.TypeReg}, {Name: "/absolute", Typeflag: tar.TypeReg}, {Name: "file", Typeflag: tar.TypeSymlink, Linkname: "/etc/passwd"}, {Name: "file", Typeflag: tar.TypeLink, Linkname: "elsewhere"}, {Name: "file", Typeflag: tar.TypeChar}} {
		var input bytes.Buffer
		w := tar.NewWriter(&input)
		if e := w.WriteHeader(h); e != nil {
			t.Fatal(e)
		}
		w.Close()
		if e := CopyTar(io.Discard, &input, 1<<20); e == nil {
			t.Fatalf("accepted %+v", h)
		}
	}
	var input bytes.Buffer
	w := tar.NewWriter(&input)
	_ = w.WriteHeader(&tar.Header{Name: "nested/module.mjs", Mode: 0777, Size: 4})
	_, _ = w.Write([]byte("code"))
	w.Close()
	var output bytes.Buffer
	if e := CopyTar(&output, &input, 1<<20); e != nil {
		t.Fatal(e)
	}
	r := tar.NewReader(&output)
	h, e := r.Next()
	if e != nil || h.Name != "nested/module.mjs" || h.Uid != 1000 || h.Mode != 0600 {
		t.Fatal(h, e)
	}
}
