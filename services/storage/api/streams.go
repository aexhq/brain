package api

import (
	"context"
	"errors"
	"io"
)

// Upload bounds every request and builds a tree with at most 512 children per
// node. It returns a durable local reference; session.flush is the remote barrier.
func (c *Client) Upload(ctx context.Context, identity Identity, r io.Reader, limit int64) (string, int64, error) {
	if limit < 0 {
		return "", 0, errors.New("negative upload limit")
	}
	var size int64
	refs := []string{}
	buffer := make([]byte, ChunkBytes)
	for {
		n, err := io.ReadFull(r, buffer)
		if err != nil && err != io.EOF && err != io.ErrUnexpectedEOF {
			return "", size, err
		}
		if n > 0 {
			size += int64(n)
			if size > limit {
				return "", size, errors.New("upload exceeds limit")
			}
			ref, e := c.PutObject(ctx, identity, Object{Data: buffer[:n]})
			if e != nil {
				return "", size, e
			}
			refs = append(refs, ref)
		}
		if err == io.EOF || err == io.ErrUnexpectedEOF {
			break
		}
	}
	for len(refs) > 512 {
		parents := make([]string, 0, (len(refs)+511)/512)
		for start := 0; start < len(refs); start += 512 {
			ref, err := c.PutObject(ctx, identity, Object{References: refs[start:min(start+512, len(refs))]})
			if err != nil {
				return "", size, err
			}
			parents = append(parents, ref)
		}
		refs = parents
	}
	ref, err := c.PutObject(ctx, identity, Object{References: refs})
	return ref, size, err
}

func (c *Client) Download(ctx context.Context, identity Identity, ref string, w io.Writer, limit int64) (int64, error) {
	var written int64
	stack := []string{ref}
	for len(stack) > 0 {
		ref = stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		o, err := c.GetObject(ctx, identity, ref)
		if err != nil {
			return written, err
		}
		if int64(len(o.Data)) > limit-written {
			return written, errors.New("download exceeds limit")
		}
		if len(o.References) > 0 && len(o.Data) > 0 {
			return written, errors.New("object is not a byte stream")
		}
		if len(o.Data) > 0 {
			n, e := w.Write(o.Data)
			written += int64(n)
			if e != nil {
				return written, e
			}
			if n != len(o.Data) {
				return written, io.ErrShortWrite
			}
		}
		for i := len(o.References) - 1; i >= 0; i-- {
			stack = append(stack, o.References[i])
		}
	}
	return written, nil
}
