package worker

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type counter struct{ calls atomic.Int64 }

func (p *counter) Run(context.Context, wire.ManagedInvocation) wire.Receipt {
	return wire.Receipt{Outcome: wire.OutcomeCompleted, Result: kernel.JSON(map[string]int64{"counter": p.calls.Add(1)})}
}

func setup(t *testing.T) (Config, *counter, wire.ManagedInvocation) {
	t.Helper()
	callback := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(503) }))
	t.Cleanup(callback.Close)
	dir := filepath.Join("..", "..", ".test-output", "worker-"+rand.Text())
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	config := Config{Directory: dir, Key: []byte("fixture-retained-worker-key-32-bytes"), Concurrency: 2, CallbackOrigins: []string{callback.URL}, Development: true}
	invocation := wire.ManagedInvocation{Execution: wire.Execution{
		Operation: wire.Operation{ID: "operation-one", SessionID: "session-one", RunID: "run-one", StepKey: "0", ArgumentsDigest: kernel.Digest([]byte("{}")),
			Kind: wire.EffectKindAction, Action: "check", Recipient: "worker", Status: wire.OperationStatusDispatched, Revision: "2"},
		Arguments: []byte("{}"), ReceiptToken: "fixture-scoped-receipt", MaxOutputBytes: 8192, DeadlineMs: uint64(time.Now().Add(time.Minute).UnixMilli()),
	}, CallbackUrl: callback.URL + "/v2/sessions/session-one/operations/operation-one/receipt",
		Package: wire.Package{ID: "package-one", Digest: "fixture-digest", Status: "ready", Manifest: wire.PackageManifest{
			Name: "example", Version: "1", Language: wire.RuntimeLanguageNode, Entrypoint: "action.mjs",
			Files: map[string]string{"action.mjs": "export default () => true"}, Lockfile: "{}", Actions: []wire.Action{},
			Secrets: []string{"SCOPED"}, Network: []string{}, MemoryBytes: 64 << 20, CpuMillis: 1000, WallMs: 1000,
		}}, Secrets: map[string]string{"SCOPED": "unique-scoped-secret-never-persisted"}}
	return config, &counter{}, invocation
}

func invoke(t *testing.T, service *Server, key []byte, invocation wire.ManagedInvocation) (int, wire.Receipt) {
	t.Helper()
	body := kernel.JSON(invocation)
	mac := hmac.New(sha256.New, key)
	_, _ = mac.Write(body)
	request := httptest.NewRequest(http.MethodPost, "/invoke", bytes.NewReader(body))
	request.Header.Set("X-Brain-Signature", "v1="+hex.EncodeToString(mac.Sum(nil)))
	request.Header.Set("X-Brain-Operation", invocation.Execution.Operation.ID)
	response := httptest.NewRecorder()
	service.ServeHTTP(response, request)
	var receipt wire.Receipt
	if response.Code == 200 {
		if err := json.Unmarshal(response.Body.Bytes(), &receipt); err != nil {
			t.Fatal(err)
		}
	}
	return response.Code, receipt
}

func TestWorkerReceiptRestartAndReplayNeverReexecutes(t *testing.T) {
	config, provider, invocation := setup(t)
	service, err := Open(config, provider)
	if err != nil {
		t.Fatal(err)
	}
	if other, err := Open(config, provider); err == nil {
		other.Close()
		t.Fatal("two workers acquired the same receipt directory")
	}
	code, receipt := invoke(t, service, config.Key, invocation)
	if code != 200 || receipt.Outcome != wire.OutcomeCompleted || provider.calls.Load() != 1 {
		t.Fatalf("initial execution: %d %+v", code, receipt)
	}
	_, duplicate := invoke(t, service, config.Key, invocation)
	if string(duplicate.Result) != string(receipt.Result) || provider.calls.Load() != 1 {
		t.Fatal("worker replay did not consume retained evidence")
	}
	var payload []byte
	if err = service.db.QueryRow("SELECT payload FROM jobs WHERE id=?", invocation.Execution.Operation.ID).Scan(&payload); err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(payload, []byte(invocation.Execution.ReceiptToken)) || bytes.Contains(payload, []byte(invocation.Secrets["SCOPED"])) {
		t.Fatal("worker persisted unencrypted credentials")
	}
	var retained wire.ManagedInvocation
	if err = service.unseal(invocation.Execution.Operation.ID, payload, &retained); err != nil || len(retained.Secrets) != 0 {
		t.Fatal("worker retained raw injected secrets")
	}
	if err = service.Close(); err != nil {
		t.Fatal(err)
	}
	service, err = Open(config, provider)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	_, recovered := invoke(t, service, config.Key, invocation)
	if string(recovered.Result) != string(receipt.Result) || provider.calls.Load() != 1 {
		t.Fatal("worker restart reexecuted a completed claim")
	}
	invocation.Execution.Arguments = []byte(`{"changed":true}`)
	if code, _ := invoke(t, service, config.Key, invocation); code != 409 || provider.calls.Load() != 1 {
		t.Fatal("worker accepted a changed request under an old operation identity")
	}
}

func TestWorkerLostClaimIsEvidenceNotPermission(t *testing.T) {
	config, provider, invocation := setup(t)
	service, err := Open(config, provider)
	if err != nil {
		t.Fatal(err)
	}
	retained := invocation
	retained.Secrets = map[string]string{}
	payload, err := service.seal(invocation.Execution.Operation.ID, retained)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = service.db.Exec("INSERT INTO jobs(id,digest,payload,created_ms) VALUES(?,?,?,0)", invocation.Execution.Operation.ID, kernel.Digest(kernel.JSON(invocation)), payload); err != nil {
		t.Fatal(err)
	}
	service.Close()
	service, err = Open(config, provider)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	code, receipt := invoke(t, service, config.Key, invocation)
	if code != 200 || receipt.Outcome != wire.OutcomeUnknown || provider.calls.Load() != 0 {
		t.Fatal("orphaned worker claim became fresh execution permission")
	}
}

func TestWorkerSignatureAndCallbackAuthority(t *testing.T) {
	config, provider, invocation := setup(t)
	service, err := Open(config, provider)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	if code, _ := invoke(t, service, []byte("wrong"), invocation); code != 401 {
		t.Fatal("worker accepted an invalid signature")
	}
	invocation.CallbackUrl += "/other"
	if code, _ := invoke(t, service, config.Key, invocation); code != 403 {
		t.Fatal("worker accepted an unrelated callback")
	}
	if provider.calls.Load() != 0 {
		t.Fatal("rejected worker request ran supplied code")
	}
}
