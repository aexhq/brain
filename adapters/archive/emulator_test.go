//go:build integration

package archive

import (
	"bytes"
	"context"
	"crypto/rand"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	dynamo "github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
	"github.com/aws/aws-sdk-go-v2/service/s3"
)

func emulatorEndpoint(t *testing.T, name string) string {
	t.Helper()
	value := os.Getenv(name)
	u, err := url.Parse(value)
	if err != nil || u.Scheme != "http" || u.Hostname() != "127.0.0.1" {
		t.Fatalf("%s must identify a real loopback emulator; this gate cannot skip or contact a live account", name)
	}
	return value
}

func TestS3EmulatorImmutableBodies(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	store, err := OpenS3(ctx, AWSConfig{Region: "us-east-1", Endpoint: emulatorEndpoint(t, "BRAIN_TEST_S3_ENDPOINT"),
		AccessKey: "brain-fixture", SecretKey: "brain-fixture-not-live", Bucket: "brain-" + strings.ToLower(rand.Text()), Prefix: "internal/", PathStyle: true})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = store.Client.CreateBucket(ctx, &s3.CreateBucketInput{Bucket: &store.Bucket}); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		for _, key := range []string{"transcript/large", "transcript/small"} {
			_ = store.Delete(cleanup, key)
		}
		if _, err := store.Client.DeleteBucket(cleanup, &s3.DeleteBucketInput{Bucket: &store.Bucket}); err != nil {
			t.Error(err)
		}
	})
	body := bytes.Repeat([]byte("real transcript bytes\n"), 150000)
	for _, data := range []struct {
		key  string
		body []byte
	}{{"transcript/large", body}, {"transcript/small", []byte("manifest")}} {
		if err = store.Put(ctx, data.key, data.body); err != nil {
			t.Fatal(err)
		}
		if err = store.Put(ctx, data.key, data.body); err != nil {
			t.Fatal(err)
		}
		result, err := store.Get(ctx, data.key, int64(len(data.body)))
		if err != nil || !bytes.Equal(result, data.body) {
			t.Fatalf("archive bytes disagree: %v", err)
		}
	}
	if err = store.Put(ctx, "transcript/small", []byte("changed!")); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("immutable conflict: %v", err)
	}
	if _, err = store.Get(ctx, "transcript/large", 128); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("oversized read: %v", err)
	}
	if err = store.Delete(ctx, "transcript/small"); err != nil {
		t.Fatal(err)
	}
	if _, err = store.Get(ctx, "transcript/small", 1024); kernel.Response(err).Code != wire.ErrorCodeUnavailable {
		t.Fatalf("missing object: %v", err)
	}
}

func TestDynamoEmulatorStoresActualChunkedTranscripts(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	store, err := OpenDynamo(ctx, AWSConfig{Region: "us-east-1", Endpoint: emulatorEndpoint(t, "BRAIN_TEST_DYNAMO_ENDPOINT"),
		AccessKey: "brainfixture", SecretKey: "brain-fixture-not-live", Table: "brain-" + rand.Text(), Prefix: "transcript/"})
	if err != nil {
		t.Fatal(err)
	}
	_, err = store.Client.CreateTable(ctx, &dynamodb.CreateTableInput{TableName: &store.Table,
		KeySchema:            []dynamo.KeySchemaElement{{AttributeName: aws.String("pk"), KeyType: dynamo.KeyTypeHash}},
		AttributeDefinitions: []dynamo.AttributeDefinition{{AttributeName: aws.String("pk"), AttributeType: dynamo.ScalarAttributeTypeS}},
		BillingMode:          dynamo.BillingModePayPerRequest})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		if _, err := store.Client.DeleteTable(cleanup, &dynamodb.DeleteTableInput{TableName: &store.Table}); err != nil {
			t.Error(err)
		}
	})
	body := bytes.Repeat([]byte("actual transcript, not an S3 pointer\n"), 60000)
	if len(body) < 400<<10 {
		t.Fatal("fixture must exceed the real DynamoDB item limit")
	}
	if err = store.Put(ctx, "session/record-block", body); err != nil {
		t.Fatal(err)
	}
	if err = store.Put(ctx, "session/record-block", body); err != nil {
		t.Fatal(err)
	}
	result, err := store.Get(ctx, "session/record-block", int64(len(body)))
	if err != nil || !bytes.Equal(result, body) {
		t.Fatalf("Dynamo transcript mismatch: %v", err)
	}
	scanned, err := store.Client.Scan(ctx, &dynamodb.ScanInput{TableName: &store.Table, ProjectionExpression: aws.String("pk,body"), Limit: aws.Int32(32)})
	if err != nil {
		t.Fatal(err)
	}
	pieces := 0
	for _, item := range scanned.Items {
		if key, ok := item["pk"].(*dynamo.AttributeValueMemberS); ok && !strings.HasSuffix(key.Value, "#manifest") {
			raw, ok := item["body"].(*dynamo.AttributeValueMemberB)
			if !ok || len(raw.Value) > DynamoPiece || len(raw.Value) == 0 {
				t.Fatal("chunk is not a bounded actual binary transcript body")
			}
			pieces++
		}
	}
	if pieces < 2 {
		t.Fatal("DynamoDB did not contain independently stored transcript chunks")
	}
	item := dynamoKey("oversized-probe")
	item["body"] = &dynamo.AttributeValueMemberB{Value: make([]byte, 401<<10)}
	if _, err = store.Client.PutItem(ctx, &dynamodb.PutItemInput{TableName: &store.Table, Item: item}); apiCode(err) != "ValidationException" {
		t.Fatalf("real item-size limit was not enforced: %v", err)
	}
	if err = store.Put(ctx, "session/record-block", []byte("different")); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("immutable manifest conflict: %v", err)
	}
	if err = store.Delete(ctx, "session/record-block"); err != nil {
		t.Fatal(err)
	}
	if _, err = store.Get(ctx, "session/record-block", int64(len(body))); kernel.Response(err).Code != wire.ErrorCodeUnavailable {
		t.Fatalf("deleted transcript is readable: %v", err)
	}
}
