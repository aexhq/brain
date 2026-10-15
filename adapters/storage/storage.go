package storage

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strconv"
	"strings"

	"github.com/aexhq/brain/adapters/archive"
	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/history"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/smithy-go"
)

type Handler struct {
	Local    *os.Root
	Object   *archive.S3
	Control  control.Store
	Blobs    history.Archive
	identity string
}

var transfers = make(chan struct{}, 8)

type arguments struct {
	Operation string  `json:"op"`
	Path      string  `json:"path"`
	Text      *string `json:"text"`
	Base64    *string `json:"base64"`
	File      *string `json:"source_file"`
	After     string  `json:"after"`
	Limit     int     `json:"limit"`
	Offset    int64   `json:"offset"`
	Bytes     int64   `json:"bytes"`
	Overwrite bool    `json:"overwrite"`
}

func OpenLocal(directory string, authority control.Store, blobs history.Archive) (*Handler, error) {
	if err := os.MkdirAll(directory, 0700); err != nil {
		return nil, err
	}
	location := filepath.Join(directory, "storage.identity")
	identity, err := os.ReadFile(location)
	if errors.Is(err, fs.ErrNotExist) {
		entries, e := os.ReadDir(directory)
		if e != nil {
			return nil, e
		}
		if len(entries) != 0 {
			return nil, kernel.Fail(wire.ErrorCodeUnsupported, "local storage has data but no retained resource identity; verified offline adoption is required")
		}
		identity = []byte(kernel.ID("agent-storage", rand.Text()))
		file, e := os.OpenFile(location, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if e != nil {
			return nil, e
		}
		_, e = file.Write(identity)
		if e == nil {
			e = file.Sync()
		}
		err = errors.Join(e, file.Close())
	}
	if err != nil {
		return nil, err
	}
	if len(identity) != 40 || !kernel.Identifier(string(identity)) {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "retained local storage identity is invalid")
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return nil, err
	}
	return &Handler{Local: root, Control: authority, Blobs: blobs, identity: string(identity)}, nil
}
func (h *Handler) Identity() string {
	if h.Local != nil {
		return "local:" + h.identity
	}
	if h.Object != nil {
		return h.Object.Identity()
	}
	return "unconfigured"
}
func (h *Handler) Close() error {
	if h.Local != nil {
		return h.Local.Close()
	}
	return nil
}

func (h *Handler) Validate(template wire.ConnectionTemplate) error {
	if h.Local == nil && h.Object == nil {
		return kernel.Fail(wire.ErrorCodeUnsupported, "agent-facing storage is not configured")
	}
	if len(template.Capabilities) == 0 {
		return kernel.Fail(wire.ErrorCodeInvalid, "storage capabilities must be explicitly admitted")
	}
	for _, capability := range template.Capabilities {
		if !slices.Contains([]string{"read", "write", "list", "stat", "delete"}, capability) {
			return kernel.Fail(wire.ErrorCodeUnsupported, "storage capability is not supported")
		}
	}
	return nil
}

func safePath(value string, root bool) bool {
	return (root && value == ".") || (value != "." && fs.ValidPath(value) && !strings.ContainsAny(value, "\\:\x00") && len(value) <= 768)
}

func (h *Handler) Invoke(ctx context.Context, tenant string, template wire.ConnectionTemplate, execution wire.Execution) (wire.Receipt, error) {
	if err := h.Validate(template); err != nil {
		return wire.Receipt{}, err
	}
	var args arguments
	if err := json.Unmarshal(execution.Arguments, &args); err != nil {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeInvalid, "invalid storage arguments")
	}
	if !slices.Contains(template.Capabilities, args.Operation) {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeForbidden, "storage operation is outside the immutable capability grant")
	}
	if args.Path == "" && args.Operation == "list" {
		args.Path = "."
	}
	if !safePath(args.Path, args.Operation == "list") {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeForbidden, "storage path must stay inside its admitted namespace")
	}
	if args.Limit == 0 {
		args.Limit = 64
	}
	if args.Limit < 1 || args.Limit > 128 || len(args.After) > 2048 || args.Offset < 0 {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeInvalid, "invalid storage page or range")
	}
	maxRead := int64(execution.MaxOutputBytes) / 2
	if args.Bytes == 0 {
		args.Bytes = maxRead
	}
	if args.Bytes < 1 || args.Bytes > maxRead || args.Offset > 1<<63-1-args.Bytes {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeInvalid, "storage read exceeds the invocation output limit")
	}
	select {
	case transfers <- struct{}{}:
		defer func() { <-transfers }()
	case <-ctx.Done():
		return wire.Receipt{Outcome: wire.OutcomeFailed, Error: kernel.Ptr("storage deadline passed before execution capacity was available")}, nil
	}
	namespace := kernel.ID("agent-storage", tenant, execution.Operation.SessionID, template.Name)
	var result any
	var err error
	if h.Local != nil {
		result, err = h.local(ctx, namespace, args, execution.Operation.SessionID)
	} else {
		result, err = h.object(ctx, namespace, args, execution.Operation.SessionID)
	}
	if err != nil {
		code := kernel.Response(err).Code
		if errors.Is(err, fs.ErrNotExist) {
			code = wire.ErrorCodeNotFound
		}
		var provider smithy.APIError
		if errors.As(err, &provider) && (provider.ErrorCode() == "NoSuchKey" || provider.ErrorCode() == "NotFound") {
			code = wire.ErrorCodeNotFound
		}
		if code == wire.ErrorCodeInternal {
			code = wire.ErrorCodeUnavailable
		}
		outcome := wire.OutcomeFailed
		if args.Operation == "write" || args.Operation == "delete" {
			outcome = wire.OutcomeUnknown
		}
		return wire.Receipt{Outcome: outcome, Error: kernel.Ptr("storage " + args.Operation + " could not be confirmed: " + string(code))}, nil
	}
	body := kernel.JSON(result)
	if uint64(len(body)) > execution.MaxOutputBytes {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("storage completed but its result exceeds the admitted output limit")}, nil
	}
	return wire.Receipt{Outcome: wire.OutcomeCompleted, Result: body}, nil
}

func (h *Handler) source(ctx context.Context, args arguments, session string) ([]byte, error) {
	count := 0
	var body []byte
	if args.Text != nil {
		count++
		body = []byte(*args.Text)
	}
	if args.Base64 != nil {
		count++
		var err error
		body, err = base64.StdEncoding.DecodeString(*args.Base64)
		if err != nil {
			return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid base64 storage input")
		}
	}
	if args.File != nil {
		count++
		if h.Control == nil || h.Blobs == nil {
			return nil, kernel.Fail(wire.ErrorCodeUnsupported, "artifact-backed storage upload is not configured")
		}
		files, err := h.Control.Files(ctx, session, 1024)
		if err != nil {
			return nil, err
		}
		found := false
		for _, file := range files {
			if file.Name == *args.File {
				found = true
				body, err = h.Blobs.Get(ctx, "blobs/"+session+"/"+file.Digest, int64(file.Size))
				if err != nil {
					return nil, err
				}
				if uint64(len(body)) != file.Size || kernel.Digest(body) != file.Digest {
					return nil, kernel.Fail(wire.ErrorCodeCorrupt, "source artifact digest mismatch")
				}
			}
		}
		if !found {
			return nil, kernel.Fail(wire.ErrorCodeNotFound, "source artifact is not in this session")
		}
	}
	if count != 1 || len(body) > 16<<20 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "storage write needs exactly one bounded text, base64 or session artifact source")
	}
	return body, nil
}

func (h *Handler) local(ctx context.Context, namespace string, args arguments, session string) (any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := h.Local.MkdirAll(namespace, 0700); err != nil {
		return nil, err
	}
	root, err := h.Local.OpenRoot(namespace)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	switch args.Operation {
	case "write":
		body, err := h.source(ctx, args, session)
		if err != nil {
			return nil, err
		}
		if err = root.MkdirAll(path.Dir(args.Path), 0700); err != nil {
			return nil, err
		}
		flags := os.O_CREATE | os.O_WRONLY
		if args.Overwrite {
			flags |= os.O_TRUNC
		} else {
			flags |= os.O_EXCL
		}
		file, err := root.OpenFile(args.Path, flags, 0600)
		if err != nil {
			return nil, err
		}
		_, err = file.Write(body)
		if err == nil {
			err = file.Sync()
		}
		err = errors.Join(err, file.Close())
		return map[string]any{"path": args.Path, "bytes": len(body), "sha256": kernel.Digest(body)}, err
	case "read":
		file, err := root.Open(args.Path)
		if err != nil {
			return nil, err
		}
		defer file.Close()
		stat, err := file.Stat()
		if err != nil || !stat.Mode().IsRegular() {
			return nil, kernel.Fail(wire.ErrorCodeForbidden, "storage read is not a regular file")
		}
		if _, err = file.Seek(args.Offset, io.SeekStart); err != nil {
			return nil, err
		}
		body, err := io.ReadAll(io.LimitReader(file, args.Bytes))
		if err != nil {
			return nil, err
		}
		return map[string]any{"path": args.Path, "offset": args.Offset, "bytes": len(body), "size": stat.Size(), "base64": base64.StdEncoding.EncodeToString(body), "more": args.Offset+int64(len(body)) < stat.Size()}, nil
	case "stat":
		stat, err := root.Lstat(args.Path)
		if err != nil {
			return nil, err
		}
		kind := "file"
		if stat.IsDir() {
			kind = "directory"
		} else if stat.Mode()&os.ModeSymlink != 0 {
			kind = "symlink"
		}
		return map[string]any{"path": args.Path, "kind": kind, "size": stat.Size(), "modified_ms": stat.ModTime().UnixMilli()}, nil
	case "list":
		file, err := root.Open(args.Path)
		if err != nil {
			return nil, err
		}
		defer file.Close()
		entries, err := file.ReadDir(4097)
		if err != nil && !errors.Is(err, io.EOF) {
			return nil, err
		}
		if len(entries) > 4096 {
			return nil, kernel.Fail(wire.ErrorCodeOverloaded, "local directory exceeds the bounded listing limit")
		}
		slices.SortFunc(entries, func(a, b fs.DirEntry) int { return strings.Compare(a.Name(), b.Name()) })
		out := []map[string]any{}
		next := ""
		for _, entry := range entries {
			if entry.Name() <= args.After {
				continue
			}
			if len(out) == args.Limit {
				next = out[len(out)-1]["name"].(string)
				break
			}
			kind := "file"
			if entry.IsDir() {
				kind = "directory"
			} else if entry.Type()&os.ModeSymlink != 0 {
				kind = "symlink"
			}
			out = append(out, map[string]any{"name": entry.Name(), "kind": kind})
		}
		return map[string]any{"entries": out, "next": next}, nil
	case "delete":
		if err := root.Remove(args.Path); err != nil && !errors.Is(err, fs.ErrNotExist) {
			return nil, err
		}
		return map[string]any{"path": args.Path, "deleted": true}, nil
	}
	return nil, kernel.Fail(wire.ErrorCodeUnsupported, "unsupported storage operation")
}

func (h *Handler) object(ctx context.Context, namespace string, args arguments, session string) (any, error) {
	client := h.Object.Client
	bucket := h.Object.Bucket
	prefix := strings.Trim(h.Object.Prefix, "/") + "/" + namespace + "/"
	key := prefix + args.Path
	switch args.Operation {
	case "write":
		body, err := h.source(ctx, args, session)
		if err != nil {
			return nil, err
		}
		request := &s3.PutObjectInput{Bucket: &bucket, Key: &key, Body: bytes.NewReader(body), ContentLength: aws.Int64(int64(len(body)))}
		if !args.Overwrite {
			request.IfNoneMatch = aws.String("*")
		}
		out, err := client.PutObject(ctx, request)
		if err != nil {
			return nil, err
		}
		return map[string]any{"path": args.Path, "bytes": len(body), "sha256": kernel.Digest(body), "etag": aws.ToString(out.ETag)}, nil
	case "read":
		rangeValue := "bytes=" + strconv.FormatInt(args.Offset, 10) + "-" + strconv.FormatInt(args.Offset+args.Bytes-1, 10)
		out, err := client.GetObject(ctx, &s3.GetObjectInput{Bucket: &bucket, Key: &key, Range: &rangeValue})
		if err != nil {
			return nil, err
		}
		defer out.Body.Close()
		body, err := io.ReadAll(io.LimitReader(out.Body, args.Bytes+1))
		if err != nil {
			return nil, err
		}
		if int64(len(body)) > args.Bytes {
			return nil, kernel.Fail(wire.ErrorCodeCorrupt, "object provider ignored the bounded range")
		}
		return map[string]any{"path": args.Path, "offset": args.Offset, "bytes": len(body), "base64": base64.StdEncoding.EncodeToString(body), "content_range": aws.ToString(out.ContentRange), "etag": aws.ToString(out.ETag)}, nil
	case "stat":
		out, err := client.HeadObject(ctx, &s3.HeadObjectInput{Bucket: &bucket, Key: &key})
		if err != nil {
			return nil, err
		}
		return map[string]any{"path": args.Path, "size": aws.ToInt64(out.ContentLength), "etag": aws.ToString(out.ETag), "media_type": aws.ToString(out.ContentType)}, nil
	case "list":
		if args.Path != "." {
			prefix += strings.TrimSuffix(args.Path, "/") + "/"
		}
		request := &s3.ListObjectsV2Input{Bucket: &bucket, Prefix: &prefix, MaxKeys: aws.Int32(int32(args.Limit))}
		if args.After != "" {
			request.ContinuationToken = &args.After
		}
		out, err := client.ListObjectsV2(ctx, request)
		if err != nil {
			return nil, err
		}
		items := []map[string]any{}
		for _, item := range out.Contents {
			object := aws.ToString(item.Key)
			if !strings.HasPrefix(object, prefix) {
				return nil, kernel.Fail(wire.ErrorCodeCorrupt, "object listing escaped the admitted prefix")
			}
			items = append(items, map[string]any{"name": strings.TrimPrefix(object, prefix), "size": aws.ToInt64(item.Size), "etag": aws.ToString(item.ETag)})
		}
		return map[string]any{"entries": items, "next": aws.ToString(out.NextContinuationToken)}, nil
	case "delete":
		_, err := client.DeleteObject(ctx, &s3.DeleteObjectInput{Bucket: &bucket, Key: &key})
		return map[string]any{"path": args.Path, "deleted": err == nil}, err
	}
	return nil, kernel.Fail(wire.ErrorCodeUnsupported, "unsupported object storage operation")
}
