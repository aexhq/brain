//go:build external

package adapters_test

import (
	"context"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	ddbstore "github.com/aexhq/brain/services/storage/internal/adapters/dynamodb"
	"github.com/aexhq/brain/services/storage/internal/adapters/postgres"
	s3store "github.com/aexhq/brain/services/storage/internal/adapters/s3"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/credentials"
	ddb "github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
	s3 "github.com/aws/aws-sdk-go-v2/service/s3"
	"os"
	"testing"
	"time"
)

func required(t *testing.T, name string) string {
	t.Helper()
	v := os.Getenv(name)
	if v == "" {
		t.Fatalf("%s is required for external adapter qualification", name)
	}
	return v
}
func TestExternalAdapters(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	access, secret := required(t, "BRAIN_TEST_ACCESS"), required(t, "BRAIN_TEST_SECRET")
	cfg := aws.Config{Region: "us-east-1", Credentials: credentials.NewStaticCredentialsProvider(access, secret, "")}
	t.Run("postgres", func(t *testing.T) {
		s, e := postgres.New(ctx, required(t, "BRAIN_TEST_POSTGRES"))
		if e != nil {
			t.Fatal(e)
		}
		// contract closes its adapter; register cleanup in the order it executes.
		contract(t, s)
	})
	t.Run("s3", func(t *testing.T) {
		endpoint := required(t, "BRAIN_TEST_S3")
		bucket := "brain-contract-" + adapters.Version()
		client := s3.NewFromConfig(cfg, func(o *s3.Options) { o.BaseEndpoint = &endpoint; o.UsePathStyle = true; o.RetryMaxAttempts = 1 })
		if _, e := client.CreateBucket(ctx, &s3.CreateBucketInput{Bucket: &bucket}); e != nil {
			t.Fatal(e)
		}
		s, e := s3store.New(ctx, s3store.Config{Bucket: bucket, Region: "us-east-1", Endpoint: endpoint, PathStyle: true, AccessKey: access, SecretKey: secret})
		if e != nil {
			t.Fatal(e)
		}
		t.Cleanup(func() {
			for _, k := range []string{"a", "b", "c"} {
				if e := s.Delete(context.Background(), "tenant/"+k); e != nil {
					t.Error(e)
				}
			}
			if _, e := client.DeleteBucket(context.Background(), &s3.DeleteBucketInput{Bucket: &bucket}); e != nil {
				t.Error(e)
			}
		})
		contract(t, s)
	})
	t.Run("dynamodb", func(t *testing.T) {
		endpoint := required(t, "BRAIN_TEST_DDB")
		table := "brain-contract-" + adapters.Version()
		client := ddb.NewFromConfig(cfg, func(o *ddb.Options) { o.BaseEndpoint = &endpoint; o.RetryMaxAttempts = 1 })
		_, e := client.CreateTable(ctx, &ddb.CreateTableInput{TableName: &table, BillingMode: types.BillingModePayPerRequest, AttributeDefinitions: []types.AttributeDefinition{{AttributeName: aws.String("pk"), AttributeType: types.ScalarAttributeTypeS}, {AttributeName: aws.String("sk"), AttributeType: types.ScalarAttributeTypeS}}, KeySchema: []types.KeySchemaElement{{AttributeName: aws.String("pk"), KeyType: types.KeyTypeHash}, {AttributeName: aws.String("sk"), KeyType: types.KeyTypeRange}}})
		if e != nil {
			t.Fatal(e)
		}
		t.Cleanup(func() {
			if _, e := client.DeleteTable(context.Background(), &ddb.DeleteTableInput{TableName: &table}); e != nil {
				t.Error(e)
			}
		})
		if e = ddb.NewTableExistsWaiter(client).Wait(ctx, &ddb.DescribeTableInput{TableName: &table}, 30*time.Second); e != nil {
			t.Fatal(e)
		}
		s, e := ddbstore.New(ctx, ddbstore.Config{Table: table, Region: "us-east-1", Endpoint: endpoint, AccessKey: access, SecretKey: secret})
		if e != nil {
			t.Fatal(e)
		}
		contract(t, s)
	})
}
