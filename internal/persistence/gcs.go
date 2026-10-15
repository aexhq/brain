package persistence

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"

	"cloud.google.com/go/storage"
	"google.golang.org/api/googleapi"
	"google.golang.org/api/option"
)

type GCSObjects struct {
	client *storage.Client
	bucket *storage.BucketHandle
	prefix string
}

func newGCS(ctx context.Context, s Settings, opts ...option.ClientOption) (*GCSObjects, error) {
	client, err := storage.NewClient(ctx, append(opts, storage.WithJSONReads())...)
	if err != nil {
		return nil, err
	}
	return &GCSObjects{client: client, bucket: client.Bucket(s.Bucket), prefix: strings.Trim(s.Prefix, "/")}, nil
}

func (s *GCSObjects) Close() error { return s.client.Close() }

func (s *GCSObjects) object(key string) (*storage.ObjectHandle, error) {
	if !validKey(key) {
		return nil, errors.New("invalid object key")
	}
	if s.prefix != "" {
		key = s.prefix + "/" + key
	}
	return s.bucket.Object(key), nil
}

func (s *GCSObjects) Put(ctx context.Context, key string, data []byte, immutable bool) error {
	object, err := s.object(key)
	if err != nil {
		return err
	}
	if len(data) > MaxObjectBytes {
		return errors.New("object exceeds limit")
	}
	if immutable {
		object = object.If(storage.Conditions{DoesNotExist: true})
	}
	w := object.NewWriter(ctx)
	// Objects are bounded to 2 MiB. Avoid a 16 MiB resumable-upload buffer;
	// the archive worker owns bounded retries of a complete object.
	w.ChunkSize = 0
	w.ContentType = "application/octet-stream"
	_, writeErr := w.Write(data)
	err = errors.Join(writeErr, w.Close())
	var api *googleapi.Error
	if immutable && errors.As(err, &api) && api.Code == 412 {
		return nil
	}
	return err
}

func (s *GCSObjects) Get(ctx context.Context, key string) ([]byte, error) {
	object, err := s.object(key)
	if err != nil {
		return nil, err
	}
	r, err := object.NewReader(ctx)
	if errors.Is(err, storage.ErrObjectNotExist) {
		return nil, fmt.Errorf("object not found: %w", os.ErrNotExist)
	}
	if err != nil {
		return nil, err
	}
	defer r.Close()
	return readObject(r)
}
