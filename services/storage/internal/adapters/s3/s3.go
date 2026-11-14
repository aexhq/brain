// Package s3 implements the same object contract for Amazon S3 and Cloudflare R2.
package s3

import (
	"bytes"
	"context"
	"errors"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aws/aws-sdk-go-v2/aws"
	awsconfig "github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	awss3 "github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/smithy-go"
	"io"
	"strings"
)

type Config struct {
	Bucket, Region, Endpoint, Prefix, AccessKey, SecretKey, SessionToken string
	PathStyle                                                            bool
}
type Store struct {
	client         *awss3.Client
	bucket, prefix string
}

func New(ctx context.Context, c Config) (*Store, error) {
	if c.Bucket == "" || (c.AccessKey == "") != (c.SecretKey == "") {
		return nil, adapters.ErrInvalid
	}
	opts := []func(*awsconfig.LoadOptions) error{}
	if c.Region != "" {
		opts = append(opts, awsconfig.WithRegion(c.Region))
	}
	if c.AccessKey != "" {
		opts = append(opts, awsconfig.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(c.AccessKey, c.SecretKey, c.SessionToken)))
	}
	cfg, err := awsconfig.LoadDefaultConfig(ctx, opts...)
	if err != nil {
		return nil, err
	}
	client := awss3.NewFromConfig(cfg, func(o *awss3.Options) {
		o.RetryMaxAttempts = 1
		o.UsePathStyle = c.PathStyle
		if c.Endpoint != "" {
			o.BaseEndpoint = aws.String(c.Endpoint)
		}
	})
	return &Store{client: client, bucket: c.Bucket, prefix: strings.Trim(c.Prefix, "/")}, nil
}
func (s *Store) key(k string) string {
	if s.prefix == "" {
		return k
	}
	return s.prefix + "/" + k
}
func classify(err error) error {
	var a smithy.APIError
	if errors.As(err, &a) {
		switch a.ErrorCode() {
		case "NoSuchKey", "NotFound":
			return adapters.ErrNotFound
		case "PreconditionFailed", "ConditionalRequestConflict":
			return adapters.ErrConflict
		}
	}
	return err
}
func (s *Store) Get(ctx context.Context, key string) (adapters.Object, error) {
	if !adapters.ValidKey(key) {
		return adapters.Object{}, adapters.ErrInvalid
	}
	out, err := s.client.GetObject(ctx, &awss3.GetObjectInput{Bucket: &s.bucket, Key: aws.String(s.key(key))})
	if err != nil {
		return adapters.Object{}, classify(err)
	}
	defer out.Body.Close()
	data, err := io.ReadAll(io.LimitReader(out.Body, (2<<20)+34))
	if err != nil {
		return adapters.Object{}, err
	}
	if len(data) < 33 || data[32] != '\n' || len(data) > (2<<20)+33 {
		return adapters.Object{}, adapters.ErrInvalid
	}
	return adapters.Object{Key: key, Data: data[33:], Version: aws.ToString(out.ETag)}, nil
}
func (s *Store) Put(ctx context.Context, key string, data []byte, c adapters.Condition) (adapters.Object, error) {
	if err := adapters.Validate(key, data, c); err != nil {
		return adapters.Object{}, err
	}
	if c.Version != "" && (len(c.Version) < 2 || c.Version[0] != '"' || c.Version[len(c.Version)-1] != '"') {
		return adapters.Object{}, adapters.ErrConflict
	}
	// S3 compares content ETags. A fresh revision prevents identical writes
	// from reusing a version and defeating the adapter's compare-and-swap.
	payload := append([]byte(adapters.Version()+"\n"), data...)
	in := &awss3.PutObjectInput{Bucket: &s.bucket, Key: aws.String(s.key(key)), Body: bytes.NewReader(payload)}
	if c.CreateOnly {
		in.IfNoneMatch = aws.String("*")
	}
	if c.Version != "" {
		in.IfMatch = &c.Version
	}
	out, err := s.client.PutObject(ctx, in)
	if err != nil {
		return adapters.Object{}, classify(err)
	}
	return adapters.Object{Key: key, Data: data, Version: aws.ToString(out.ETag)}, nil
}
func (s *Store) Delete(ctx context.Context, key string) error {
	if !adapters.ValidKey(key) {
		return adapters.ErrInvalid
	}
	_, err := s.client.DeleteObject(ctx, &awss3.DeleteObjectInput{Bucket: &s.bucket, Key: aws.String(s.key(key))})
	return classify(err)
}
func (s *Store) List(ctx context.Context, prefix, after string, limit int) (adapters.Page, error) {
	if err := adapters.ValidateList(prefix, after, limit); err != nil {
		return adapters.Page{}, err
	}
	in := &awss3.ListObjectsV2Input{Bucket: &s.bucket, Prefix: aws.String(s.key(prefix)), MaxKeys: aws.Int32(int32(limit))}
	if after != "" {
		in.StartAfter = aws.String(s.key(after))
	}
	out, err := s.client.ListObjectsV2(ctx, in)
	if err != nil {
		return adapters.Page{}, classify(err)
	}
	p := adapters.Page{Objects: []adapters.Object{}}
	for _, o := range out.Contents {
		key := aws.ToString(o.Key)
		if s.prefix != "" {
			key = strings.TrimPrefix(key, s.prefix+"/")
		}
		p.Objects = append(p.Objects, adapters.Object{Key: key, Version: aws.ToString(o.ETag)})
	}
	if aws.ToBool(out.IsTruncated) && len(p.Objects) > 0 {
		p.Next = p.Objects[len(p.Objects)-1].Key
	}
	return p, nil
}
func (s *Store) Close() error { return nil }
