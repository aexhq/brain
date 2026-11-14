package dynamodb

import (
	"context"
	"errors"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aws/aws-sdk-go-v2/aws"
	awsconfig "github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	ddb "github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
	"strings"
)

// The first key component is a namespace partition, the rest an ordered sort key.
// List always selects a namespace; it never performs an unbounded table scan.
type Store struct {
	client *ddb.Client
	table  string
}
type Config struct{ Table, Region, Endpoint, AccessKey, SecretKey string }

func New(ctx context.Context, c Config) (*Store, error) {
	if c.Table == "" || (c.AccessKey == "") != (c.SecretKey == "") {
		return nil, adapters.ErrInvalid
	}
	opts := []func(*awsconfig.LoadOptions) error{}
	if c.Region != "" {
		opts = append(opts, awsconfig.WithRegion(c.Region))
	}
	if c.AccessKey != "" {
		opts = append(opts, awsconfig.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(c.AccessKey, c.SecretKey, "")))
	}
	cfg, err := awsconfig.LoadDefaultConfig(ctx, opts...)
	if err != nil {
		return nil, err
	}
	return &Store{client: ddb.NewFromConfig(cfg, func(o *ddb.Options) {
		o.RetryMaxAttempts = 1
		if c.Endpoint != "" {
			o.BaseEndpoint = &c.Endpoint
		}
	}), table: c.Table}, nil
}
func str(s string) types.AttributeValue { return &types.AttributeValueMemberS{Value: s} }
func key(k string) (map[string]types.AttributeValue, error) {
	if !adapters.ValidKey(k) {
		return nil, adapters.ErrInvalid
	}
	ns, rest, ok := strings.Cut(k, "/")
	if !ok || rest == "" {
		return nil, adapters.ErrInvalid
	}
	return map[string]types.AttributeValue{"pk": str(ns), "sk": str(rest)}, nil
}
func decode(item map[string]types.AttributeValue) (adapters.Object, error) {
	pk, p := item["pk"].(*types.AttributeValueMemberS)
	sk, s := item["sk"].(*types.AttributeValueMemberS)
	v, ok := item["revision"].(*types.AttributeValueMemberS)
	data, d := item["payload"].(*types.AttributeValueMemberB)
	if !p || !s || !ok || !d {
		return adapters.Object{}, errors.New("corrupt DynamoDB object")
	}
	return adapters.Object{Key: pk.Value + "/" + sk.Value, Version: v.Value, Data: data.Value}, nil
}
func classify(err error) error {
	var c *types.ConditionalCheckFailedException
	if errors.As(err, &c) {
		return adapters.ErrConflict
	}
	return err
}
func (s *Store) Get(ctx context.Context, k string) (adapters.Object, error) {
	keys, err := key(k)
	if err != nil {
		return adapters.Object{}, err
	}
	out, err := s.client.GetItem(ctx, &ddb.GetItemInput{TableName: &s.table, Key: keys, ConsistentRead: aws.Bool(true)})
	if err != nil {
		return adapters.Object{}, err
	}
	if len(out.Item) == 0 {
		return adapters.Object{}, adapters.ErrNotFound
	}
	return decode(out.Item)
}
func (s *Store) Put(ctx context.Context, k string, data []byte, c adapters.Condition) (adapters.Object, error) {
	if err := adapters.Validate(k, data, c); err != nil {
		return adapters.Object{}, err
	}
	if data == nil {
		data = []byte{}
	}
	if len(data) > 384<<10 {
		return adapters.Object{}, adapters.ErrInvalid
	}
	item, err := key(k)
	if err != nil {
		return adapters.Object{}, err
	}
	v := adapters.Version()
	item["revision"] = str(v)
	item["payload"] = &types.AttributeValueMemberB{Value: data}
	in := &ddb.PutItemInput{TableName: &s.table, Item: item}
	if c.CreateOnly {
		in.ConditionExpression = aws.String("attribute_not_exists(pk)")
	}
	if c.Version != "" {
		in.ConditionExpression = aws.String("revision = :revision")
		in.ExpressionAttributeValues = map[string]types.AttributeValue{":revision": str(c.Version)}
	}
	_, err = s.client.PutItem(ctx, in)
	if err != nil {
		return adapters.Object{}, classify(err)
	}
	return adapters.Object{Key: k, Version: v, Data: data}, nil
}
func (s *Store) Delete(ctx context.Context, k string) error {
	keys, err := key(k)
	if err != nil {
		return err
	}
	_, err = s.client.DeleteItem(ctx, &ddb.DeleteItemInput{TableName: &s.table, Key: keys})
	return err
}
func (s *Store) List(ctx context.Context, prefix, after string, limit int) (adapters.Page, error) {
	if err := adapters.ValidateList(prefix, after, limit); err != nil {
		return adapters.Page{}, err
	}
	ns, rest, ok := strings.Cut(prefix, "/")
	if !ok || ns == "" {
		return adapters.Page{}, adapters.ErrInvalid
	}
	values := map[string]types.AttributeValue{":namespace": str(ns)}
	expression := "pk = :namespace"
	if rest != "" {
		expression += " AND begins_with(sk, :prefix)"
		values[":prefix"] = str(rest)
	}
	in := &ddb.QueryInput{TableName: &s.table, KeyConditionExpression: &expression, ExpressionAttributeValues: values, ConsistentRead: aws.Bool(true), Limit: aws.Int32(int32(limit))}
	if after != "" {
		k, err := key(after)
		if err != nil {
			return adapters.Page{}, err
		}
		if !strings.HasPrefix(after, ns+"/") {
			return adapters.Page{}, adapters.ErrInvalid
		}
		in.ExclusiveStartKey = k
	}
	out, err := s.client.Query(ctx, in)
	if err != nil {
		return adapters.Page{}, err
	}
	p := adapters.Page{Objects: []adapters.Object{}}
	for _, item := range out.Items {
		o, err := decode(item)
		if err != nil {
			return p, err
		}
		o.Data = nil
		p.Objects = append(p.Objects, o)
	}
	if len(out.LastEvaluatedKey) > 0 && len(p.Objects) > 0 {
		p.Next = p.Objects[len(p.Objects)-1].Key
	}
	return p, nil
}
func (s *Store) Close() error { return nil }
