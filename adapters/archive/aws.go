package archive

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strconv"
	"strings"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	dynamotypes "github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/smithy-go"
)

type AWSConfig struct {
	Region    string
	Endpoint  string
	AccessKey string
	SecretKey string
	Bucket    string
	Table     string
	Prefix    string
	PathStyle bool
}

func awsConfig(ctx context.Context, settings AWSConfig) (aws.Config, error) {
	options := []func(*config.LoadOptions) error{config.WithRegion(settings.Region), config.WithRetryMaxAttempts(1)}
	if settings.AccessKey != "" || settings.SecretKey != "" {
		if settings.AccessKey == "" || settings.SecretKey == "" {
			return aws.Config{}, kernel.Fail(wire.ErrorCodeInvalid, "both archive credential fields are required")
		}
		options = append(options, config.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(settings.AccessKey, settings.SecretKey, "")))
	}
	return config.LoadDefaultConfig(ctx, options...)
}

type S3 struct {
	Client *s3.Client
	Bucket string
	Prefix string
}

func (s *S3) Identity() string {
	options := s.Client.Options()
	return "s3:" + kernel.ID(s.Bucket, s.Prefix, aws.ToString(options.BaseEndpoint), options.Region)
}

func OpenS3(ctx context.Context, settings AWSConfig) (*S3, error) {
	if settings.Bucket == "" || (settings.Prefix != "" && !validKey(strings.TrimSuffix(settings.Prefix, "/"))) {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid internal object-store bucket or prefix")
	}
	cfg, err := awsConfig(ctx, settings)
	if err != nil {
		return nil, err
	}
	client := s3.NewFromConfig(cfg, func(options *s3.Options) {
		options.UsePathStyle = settings.PathStyle
		if settings.Endpoint != "" {
			options.BaseEndpoint = aws.String(settings.Endpoint)
		}
		// R2 does not implement optional AWS trailing checksums uniformly.
		options.RequestChecksumCalculation = aws.RequestChecksumCalculationWhenRequired
		options.ResponseChecksumValidation = aws.ResponseChecksumValidationWhenRequired
	})
	return &S3{client, settings.Bucket, strings.TrimSuffix(settings.Prefix, "/")}, nil
}

func (s *S3) key(key string) (string, error) {
	if !validKey(key) {
		return "", kernel.Fail(wire.ErrorCodeInvalid, "invalid object key")
	}
	if s.Prefix != "" {
		key = s.Prefix + "/" + key
	}
	return key, nil
}
func apiCode(err error) string {
	var api smithy.APIError
	if errors.As(err, &api) {
		return api.ErrorCode()
	}
	return ""
}

func (s *S3) Put(ctx context.Context, key string, body []byte) error {
	object, err := s.key(key)
	if err != nil {
		return err
	}
	_, err = s.Client.PutObject(ctx, &s3.PutObjectInput{Bucket: &s.Bucket, Key: &object, Body: bytes.NewReader(body), ContentLength: aws.Int64(int64(len(body))), IfNoneMatch: aws.String("*"), Metadata: map[string]string{"sha256": kernel.Digest(body)}})
	if apiCode(err) == "PreconditionFailed" || apiCode(err) == "ConditionalRequestConflict" {
		prior, e := s.Get(ctx, key, int64(len(body)))
		if e != nil {
			return e
		}
		if kernel.Digest(prior) != kernel.Digest(body) {
			return kernel.Fail(wire.ErrorCodeCorrupt, "immutable S3/R2 object conflict")
		}
		return nil
	}
	return err
}
func (s *S3) Get(ctx context.Context, key string, limit int64) ([]byte, error) {
	object, err := s.key(key)
	if err != nil {
		return nil, err
	}
	response, err := s.Client.GetObject(ctx, &s3.GetObjectInput{Bucket: &s.Bucket, Key: &object})
	if err != nil {
		if apiCode(err) == "NoSuchKey" || apiCode(err) == "NotFound" {
			return nil, kernel.Fail(wire.ErrorCodeUnavailable, "archive object is missing")
		}
		return nil, err
	}
	defer response.Body.Close()
	if response.ContentLength != nil && *response.ContentLength > limit {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "archive object exceeds its declared bound")
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if int64(len(body)) > limit {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "archive object exceeds its declared bound")
	}
	return body, err
}
func (s *S3) Delete(ctx context.Context, key string) error {
	object, err := s.key(key)
	if err != nil {
		return err
	}
	_, err = s.Client.DeleteObject(ctx, &s3.DeleteObjectInput{Bucket: &s.Bucket, Key: &object})
	return err
}

const DynamoPiece = 256 << 10
const DynamoMaximum = 64 << 20

type Dynamo struct {
	Client *dynamodb.Client
	Table  string
	Prefix string
}

func (d *Dynamo) Identity() string {
	options := d.Client.Options()
	return "dynamodb:" + kernel.ID(d.Table, d.Prefix, aws.ToString(options.BaseEndpoint), options.Region)
}

type dynamoManifest struct {
	Version int      `json:"version"`
	Digest  string   `json:"digest"`
	Bytes   int      `json:"bytes"`
	Pieces  []string `json:"pieces"`
}

func OpenDynamo(ctx context.Context, settings AWSConfig) (*Dynamo, error) {
	if settings.Table == "" {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "DynamoDB transcript table is required")
	}
	cfg, err := awsConfig(ctx, settings)
	if err != nil {
		return nil, err
	}
	client := dynamodb.NewFromConfig(cfg, func(options *dynamodb.Options) {
		if settings.Endpoint != "" {
			options.BaseEndpoint = aws.String(settings.Endpoint)
		}
	})
	return &Dynamo{client, settings.Table, settings.Prefix}, nil
}
func dynamoKey(value string) map[string]dynamotypes.AttributeValue {
	return map[string]dynamotypes.AttributeValue{"pk": &dynamotypes.AttributeValueMemberS{Value: value}}
}

func (d *Dynamo) putItem(ctx context.Context, key string, body []byte) error {
	digest := kernel.Digest(body)
	item := dynamoKey(key)
	item["body"] = &dynamotypes.AttributeValueMemberB{Value: body}
	item["sha256"] = &dynamotypes.AttributeValueMemberS{Value: digest}
	_, err := d.Client.PutItem(ctx, &dynamodb.PutItemInput{TableName: &d.Table, Item: item, ConditionExpression: aws.String("attribute_not_exists(pk)")})
	if apiCode(err) == "ConditionalCheckFailedException" {
		prior, e := d.getItem(ctx, key, int64(len(body)))
		if e != nil {
			return e
		}
		if kernel.Digest(prior) != digest {
			return kernel.Fail(wire.ErrorCodeCorrupt, "immutable DynamoDB item conflict")
		}
		return nil
	}
	return err
}

func (d *Dynamo) getItem(ctx context.Context, key string, limit int64) ([]byte, error) {
	response, err := d.Client.GetItem(ctx, &dynamodb.GetItemInput{TableName: &d.Table, Key: dynamoKey(key), ConsistentRead: aws.Bool(true)})
	if err != nil {
		return nil, err
	}
	if len(response.Item) == 0 {
		return nil, kernel.Fail(wire.ErrorCodeUnavailable, "DynamoDB transcript item is missing")
	}
	body, ok := response.Item["body"].(*dynamotypes.AttributeValueMemberB)
	digest, hashOK := response.Item["sha256"].(*dynamotypes.AttributeValueMemberS)
	if !ok || !hashOK || int64(len(body.Value)) > limit || kernel.Digest(body.Value) != digest.Value {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "invalid DynamoDB transcript item")
	}
	return body.Value, nil
}

func (d *Dynamo) Put(ctx context.Context, key string, body []byte) error {
	if !validKey(key) || len(key)+len(d.Prefix) > 1000 || len(body) > DynamoMaximum {
		return kernel.Fail(wire.ErrorCodeInvalid, "DynamoDB archive key or body exceeds its bound")
	}
	base := d.Prefix + key
	m := dynamoManifest{Version: 1, Digest: kernel.Digest(body), Bytes: len(body), Pieces: []string{}}
	for offset := 0; offset < len(body); offset += DynamoPiece {
		piece := body[offset:min(offset+DynamoPiece, len(body))]
		digest := kernel.Digest(piece)
		pieceKey := base + "#" + strconv.Itoa(len(m.Pieces)) + "#" + digest
		if err := d.putItem(ctx, pieceKey, piece); err != nil {
			return err
		}
		m.Pieces = append(m.Pieces, digest)
	}
	// Only publish the manifest after all actual binary transcript chunks exist.
	return d.putItem(ctx, base+"#manifest", kernel.JSON(m))
}

func (d *Dynamo) Get(ctx context.Context, key string, limit int64) ([]byte, error) {
	if !validKey(key) {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid DynamoDB key")
	}
	base := d.Prefix + key
	body, err := d.getItem(ctx, base+"#manifest", 32768)
	if err != nil {
		return nil, err
	}
	var m dynamoManifest
	if err = json.Unmarshal(body, &m); err != nil || m.Version != 1 || m.Bytes < 0 || m.Bytes > DynamoMaximum || int64(m.Bytes) > limit ||
		len(m.Pieces) != (m.Bytes+DynamoPiece-1)/DynamoPiece {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "invalid DynamoDB transcript manifest")
	}
	out := make([]byte, 0, m.Bytes)
	for i, digest := range m.Pieces {
		part, err := d.getItem(ctx, base+"#"+strconv.Itoa(i)+"#"+digest, DynamoPiece)
		if err != nil {
			return nil, err
		}
		if kernel.Digest(part) != digest || len(part) != min(DynamoPiece, m.Bytes-i*DynamoPiece) {
			return nil, kernel.Fail(wire.ErrorCodeCorrupt, "DynamoDB transcript piece mismatch")
		}
		out = append(out, part...)
	}
	if len(out) != m.Bytes || kernel.Digest(out) != m.Digest {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "DynamoDB transcript body mismatch")
	}
	return out, nil
}

func (d *Dynamo) Delete(ctx context.Context, key string) error {
	if !validKey(key) {
		return kernel.Fail(wire.ErrorCodeInvalid, "invalid DynamoDB key")
	}
	base := d.Prefix + key
	body, err := d.getItem(ctx, base+"#manifest", 32768)
	if kernel.Response(err).Code == wire.ErrorCodeUnavailable {
		return nil
	}
	if err != nil {
		return err
	}
	var m dynamoManifest
	if err = json.Unmarshal(body, &m); err != nil || len(m.Pieces) > DynamoMaximum/DynamoPiece {
		return kernel.Fail(wire.ErrorCodeCorrupt, "invalid DynamoDB deletion manifest")
	}
	// Keep the deletion inventory until all chunks are gone. A caller with an
	// already committed tombstone can resume cleanup after any interrupted delete.
	for i, digest := range m.Pieces {
		_, err = d.Client.DeleteItem(ctx, &dynamodb.DeleteItemInput{TableName: &d.Table, Key: dynamoKey(fmt.Sprintf("%s#%d#%s", base, i, digest))})
		if err != nil {
			return err
		}
	}
	_, err = d.Client.DeleteItem(ctx, &dynamodb.DeleteItemInput{TableName: &d.Table, Key: dynamoKey(base + "#manifest")})
	return err
}
