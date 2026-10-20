// Package persistence publishes immutable checkpoints outside the hot path.
package persistence

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"strings"

	"github.com/aws/aws-sdk-go-v2/aws"
	awsconfig "github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/smithy-go"
)

const MaxObjectBytes = 2 << 20

type Objects interface {
	DeletePrefix(context.Context, string) error
	Put(context.Context, string, []byte, bool) error
	Get(context.Context, string) ([]byte, error)
}

type Settings struct {
	Provider        string `json:"provider"`
	Directory       string `json:"directory,omitempty"`
	Bucket          string `json:"bucket,omitempty"`
	Region          string `json:"region,omitempty"`
	Endpoint        string `json:"endpoint,omitempty"`
	Prefix          string `json:"prefix,omitempty"`
	AccessKeyEnv    string `json:"access_key_env,omitempty"`
	SecretKeyEnv    string `json:"secret_key_env,omitempty"`
	PostgresEnv     string `json:"postgres_env,omitempty"`
	IntervalSeconds int    `json:"interval_seconds,omitempty"`
	Queue           int    `json:"queue,omitempty"`
}

func (s *Settings) Validate() error {
	if s.IntervalSeconds == 0 {
		s.IntervalSeconds = 5
	}
	if s.Queue == 0 {
		s.Queue = 128
	}
	if s.IntervalSeconds < 1 || s.IntervalSeconds > 3600 || s.Queue < 1 || s.Queue > 10000 {
		return errors.New("invalid synchronization limits")
	}
	if s.Provider != "filesystem" && s.Provider != "s3" && s.Provider != "r2" && s.Provider != "gcs" {
		return errors.New("unknown storage provider")
	}
	if s.Provider == "filesystem" && s.Directory == "" {
		return errors.New("filesystem storage requires directory")
	}
	if s.Provider != "filesystem" && s.Bucket == "" {
		return errors.New("storage bucket is required")
	}
	if s.Endpoint != "" {
		u, err := url.Parse(s.Endpoint)
		if err != nil || (u.Scheme != "https" && u.Scheme != "http") || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
			return errors.New("invalid storage endpoint")
		}
	}
	if s.Provider == "r2" && (s.Endpoint == "" || s.AccessKeyEnv == "" || s.SecretKeyEnv == "") {
		return errors.New("R2 requires endpoint and credentials")
	}
	if s.Provider == "gcs" && (s.Endpoint != "" || s.Region != "" || s.AccessKeyEnv != "" || s.SecretKeyEnv != "") {
		return errors.New("GCS uses application default credentials; S3 settings are not supported")
	}
	if s.AccessKeyEnv != "" || s.SecretKeyEnv != "" {
		if os.Getenv(s.AccessKeyEnv) == "" || os.Getenv(s.SecretKeyEnv) == "" {
			return errors.New("storage credentials are unavailable")
		}
	}
	if s.PostgresEnv != "" && os.Getenv(s.PostgresEnv) == "" {
		return errors.New("PostgreSQL connection string is missing")
	}
	if s.Prefix != "" && !validKey(strings.TrimSuffix(s.Prefix, "/")) {
		return errors.New("invalid storage prefix")
	}
	return nil
}

func validKey(key string) bool {
	return key != "" && path.Clean(key) == key && !strings.HasPrefix(key, "/") && !strings.Contains(key, "\\") && filepath.IsLocal(filepath.FromSlash(key))
}

type FileObjects struct{ Root string }

func (s FileObjects) Put(ctx context.Context, key string, data []byte, immutable bool) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if !validKey(key) || len(data) > MaxObjectBytes {
		return errors.New("invalid object key or size")
	}
	p := filepath.Join(s.Root, filepath.FromSlash(key))
	if err := os.MkdirAll(filepath.Dir(p), 0700); err != nil {
		return err
	}
	f, err := os.CreateTemp(filepath.Dir(p), ".upload-")
	if err != nil {
		return err
	}
	tmp := f.Name()
	defer os.Remove(tmp)
	if _, err := f.Write(data); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	if immutable {
		err := os.Link(tmp, p)
		if errors.Is(err, os.ErrExist) {
			return nil
		}
		return err
	}
	return os.Rename(tmp, p)
}
func (s FileObjects) Get(ctx context.Context, key string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !validKey(key) {
		return nil, errors.New("invalid object key")
	}
	f, err := os.Open(filepath.Join(s.Root, filepath.FromSlash(key)))
	if err != nil {
		return nil, err
	}
	defer f.Close()
	return readObject(f)
}
func readObject(r io.Reader) ([]byte, error) {
	b, err := io.ReadAll(io.LimitReader(r, MaxObjectBytes+1))
	if len(b) > MaxObjectBytes {
		return nil, errors.New("object exceeds limit")
	}
	return b, err
}

type S3Objects struct {
	client         *s3.Client
	bucket, prefix string
}

func NewObjects(ctx context.Context, s Settings) (Objects, error) {
	if err := s.Validate(); err != nil {
		return nil, err
	}
	if s.Provider == "gcs" {
		return newGCS(ctx, s)
	}
	if s.Prefix != "" && !validKey(strings.TrimSuffix(s.Prefix, "/")) {
		return nil, errors.New("invalid storage prefix")
	}
	if s.Provider == "filesystem" {
		if s.Directory == "" {
			return nil, errors.New("filesystem storage requires directory")
		}
		root, err := filepath.Abs(s.Directory)
		if err != nil {
			return nil, err
		}
		if err := os.MkdirAll(root, 0700); err != nil {
			return nil, err
		}
		return FileObjects{Root: root}, nil
	}
	if s.Provider != "s3" && s.Provider != "r2" {
		return nil, errors.New("unknown storage provider")
	}
	if s.Bucket == "" {
		return nil, errors.New("storage bucket is required")
	}
	if s.Provider == "r2" {
		if s.Endpoint == "" || s.AccessKeyEnv == "" || s.SecretKeyEnv == "" {
			return nil, errors.New("R2 requires endpoint and credentials")
		}
		if s.Region == "" {
			s.Region = "auto"
		}
	}
	options := []func(*awsconfig.LoadOptions) error{awsconfig.WithRegion(s.Region), awsconfig.WithRetryMaxAttempts(3)}
	if s.AccessKeyEnv != "" || s.SecretKeyEnv != "" {
		access, secret := os.Getenv(s.AccessKeyEnv), os.Getenv(s.SecretKeyEnv)
		if access == "" || secret == "" {
			return nil, errors.New("storage credentials are unavailable")
		}
		options = append(options, awsconfig.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(access, secret, "")))
	}
	cfg, err := awsconfig.LoadDefaultConfig(ctx, options...)
	if err != nil {
		return nil, err
	}
	client := s3.NewFromConfig(cfg, func(o *s3.Options) {
		if s.Endpoint != "" {
			o.BaseEndpoint = aws.String(s.Endpoint)
			o.UsePathStyle = true
		}
		if s.Provider == "r2" {
			o.RequestChecksumCalculation = aws.RequestChecksumCalculationWhenRequired
			o.ResponseChecksumValidation = aws.ResponseChecksumValidationWhenRequired
		}
	})
	return &S3Objects{client: client, bucket: s.Bucket, prefix: strings.Trim(s.Prefix, "/")}, nil
}
func (s *S3Objects) key(key string) (string, error) {
	if !validKey(key) {
		return "", errors.New("invalid object key")
	}
	if s.prefix != "" {
		return s.prefix + "/" + key, nil
	}
	return key, nil
}
func (s *S3Objects) Put(ctx context.Context, key string, data []byte, immutable bool) error {
	key, err := s.key(key)
	if err != nil {
		return err
	}
	if len(data) > MaxObjectBytes {
		return errors.New("object exceeds limit")
	}
	input := &s3.PutObjectInput{Bucket: aws.String(s.bucket), Key: aws.String(key), Body: bytes.NewReader(data), ContentLength: aws.Int64(int64(len(data)))}
	if immutable {
		input.IfNoneMatch = aws.String("*")
	}
	_, err = s.client.PutObject(ctx, input)
	var api smithy.APIError
	if immutable && errors.As(err, &api) && api.ErrorCode() == "PreconditionFailed" {
		return nil
	}
	return err
}
func (s *S3Objects) Get(ctx context.Context, key string) ([]byte, error) {
	key, err := s.key(key)
	if err != nil {
		return nil, err
	}
	out, err := s.client.GetObject(ctx, &s3.GetObjectInput{Bucket: aws.String(s.bucket), Key: aws.String(key)})
	if err != nil {
		var api smithy.APIError
		if errors.As(err, &api) && (api.ErrorCode() == "NoSuchKey" || api.ErrorCode() == "NotFound") {
			return nil, fmt.Errorf("object not found: %w", os.ErrNotExist)
		}
		return nil, err
	}
	defer out.Body.Close()
	return readObject(out.Body)
}
