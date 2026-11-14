// Package pi installs Pi's protocol over an isolated Sandbox environment.
package pi

import (
	"archive/tar"
	"context"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/aexhq/brain/runtimes/host"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/sandbox/api"
)

func Factory(client *api.Client, maxBytes int64) *harness.ProcessFactory {
	return &harness.ProcessFactory{Steering: true, MaxShared: 1, MaxActiveTurns: 1, Launch: func(ctx context.Context, o harness.Options) (harness.Connection, error) {
		env, err := client.Create(ctx, api.Create{Tenant: core.Tenant(ctx), Session: o.SessionID, Profile: "pi"})
		if err != nil {
			return nil, err
		}
		c := &connection{client: client, id: env.ID, local: o.Dir, maxBytes: maxBytes}
		ok := false
		defer func() {
			if !ok {
				_ = c.Close()
			}
		}()
		if err = importNative(ctx, c, o.Dir); err != nil {
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
	client    *api.Client
	id, local string
	maxBytes  int64
	once      sync.Once
	err       error
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
func importNative(ctx context.Context, c *connection, dir string) error {
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	reader, writer := io.Pipe()
	done := make(chan error, 1)
	go func() {
		tw := tar.NewWriter(writer)
		var size int64
		err := filepath.WalkDir(dir, func(p string, d fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			rel, err := filepath.Rel(dir, p)
			if err != nil {
				return err
			}
			if rel == "." || d.IsDir() {
				return nil
			}
			info, err := d.Info()
			if err != nil {
				return err
			}
			if !info.Mode().IsRegular() {
				return errors.New("native state contains a non-regular file")
			}
			size += info.Size()
			if size > c.maxBytes {
				return errors.New("native state exceeds limit")
			}
			f, err := os.Open(p)
			if err != nil {
				return err
			}
			defer f.Close()
			if err = tw.WriteHeader(&tar.Header{Name: "native/" + filepath.ToSlash(rel), Mode: 0600, Size: info.Size()}); err != nil {
				return err
			}
			_, err = io.Copy(tw, f)
			return err
		})
		err = errors.Join(err, tw.Close())
		_ = writer.CloseWithError(err)
		done <- err
	}()
	err := c.client.Import(ctx, c.id, reader)
	_ = reader.CloseWithError(err)
	return errors.Join(err, <-done)
}
func (c *connection) Export(ctx context.Context, files []core.NativeFile) error {
	root, err := os.OpenRoot(c.local)
	if err != nil {
		return err
	}
	defer root.Close()
	wanted := map[string]int64{}
	for _, f := range files {
		if !filepath.IsLocal(f.Path) || f.Size < 0 {
			return errors.New("invalid Pi state file")
		}
		wanted["native/"+filepath.ToSlash(f.Path)] = f.Size
	}
	reader, writer := io.Pipe()
	done := make(chan error, 1)
	go func() {
		err := c.client.Export(ctx, c.id, writer, c.maxBytes)
		_ = writer.CloseWithError(err)
		done <- err
	}()
	tr := tar.NewReader(reader)
	copyErr := func() error {
		for {
			h, e := tr.Next()
			if e == io.EOF {
				break
			}
			if e != nil {
				return e
			}
			name := strings.TrimPrefix(h.Name, "./")
			size, ok := wanted[name]
			if !ok {
				continue
			}
			if h.Typeflag != tar.TypeReg || h.Size != size {
				return errors.New("inconsistent Pi checkpoint export")
			}
			rel := strings.TrimPrefix(name, "native/")
			if e = root.MkdirAll(filepath.Dir(rel), 0700); e != nil {
				return e
			}
			out, e := root.OpenFile(rel, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0600)
			if e != nil {
				return e
			}
			_, e = io.CopyN(out, tr, size)
			if e == nil {
				e = out.Sync()
			}
			e = errors.Join(e, out.Close())
			if e != nil {
				return e
			}
			delete(wanted, name)
		}
		if len(wanted) > 0 {
			return errors.New("Pi checkpoint export is incomplete")
		}
		return nil
	}()
	_ = reader.CloseWithError(copyErr)
	return errors.Join(copyErr, <-done)
}
