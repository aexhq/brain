package persistence

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"cloud.google.com/go/storage"
	"github.com/aexhq/brain/internal/core"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"google.golang.org/api/iterator"
)

func (s FileObjects) DeletePrefix(ctx context.Context, prefix string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if !strings.HasSuffix(prefix, "/") || !validKey(strings.TrimSuffix(prefix, "/")) {
		return errors.New("invalid deletion prefix")
	}
	return os.RemoveAll(filepath.Join(s.Root, filepath.FromSlash(prefix)))
}

func (s *S3Objects) DeletePrefix(ctx context.Context, prefix string) error {
	if !strings.HasSuffix(prefix, "/") {
		return errors.New("invalid deletion prefix")
	}
	key, err := s.key(strings.TrimSuffix(prefix, "/"))
	if err != nil {
		return err
	}
	// Fetch the first page again after deletion: no continuation cursor can skip keys.
	for {
		page, err := s.client.ListObjectsV2(ctx, &s3.ListObjectsV2Input{Bucket: aws.String(s.bucket), Prefix: aws.String(key + "/"), MaxKeys: aws.Int32(1000)})
		if err != nil {
			return err
		}
		if len(page.Contents) == 0 {
			return nil
		}
		for _, object := range page.Contents {
			if _, err := s.client.DeleteObject(ctx, &s3.DeleteObjectInput{Bucket: aws.String(s.bucket), Key: object.Key}); err != nil {
				return err
			}
		}
	}
}

func (s *GCSObjects) DeletePrefix(ctx context.Context, prefix string) error {
	if !strings.HasSuffix(prefix, "/") || !validKey(strings.TrimSuffix(prefix, "/")) {
		return errors.New("invalid deletion prefix")
	}
	if s.prefix != "" {
		prefix = s.prefix + "/" + prefix
	}
	objects := s.bucket.Objects(ctx, &storage.Query{Prefix: prefix})
	for {
		object, err := objects.Next()
		if err == iterator.Done {
			return nil
		}
		if err != nil {
			return err
		}
		if err := s.bucket.Object(object.Name).Delete(ctx); err != nil && !errors.Is(err, storage.ErrObjectNotExist) {
			return err
		}
	}
}

func (a *Archive) deleted(ctx context.Context, id string) (bool, error) {
	_, err := a.Objects.Get(ctx, "deleted/"+id)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	return err == nil, err
}

func (a *Archive) Delete(ctx context.Context, tenant, id string) error {
	if !core.ValidID(id) || tenant == "" {
		return core.E(core.Invalid, "invalid deletion identity")
	}
	if err := a.Objects.Put(ctx, "deleted/"+id, []byte(tenant), true); err != nil {
		return err
	}
	if a.Metadata != nil {
		if err := a.Metadata.Delete(ctx, tenant, id); err != nil {
			return err
		}
	}
	return a.Objects.DeletePrefix(ctx, "sessions/"+id+"/")
}
