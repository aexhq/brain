package worker

import (
	"bytes"
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/aexhq/brain/adapters/network"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/validation"
	_ "modernc.org/sqlite"
)

type Provider interface {
	Run(context.Context, wire.ManagedInvocation) wire.Receipt
}

type Config struct {
	Directory       string
	Key             []byte
	Concurrency     int
	CallbackOrigins []string
	Development     bool
}

type Server struct {
	config    Config
	db        *sql.DB
	lockDB    *sql.DB
	lock      *sql.Tx
	cipher    cipher.AEAD
	provider  Provider
	validator *validation.Validator
	client    *http.Client
	slots     chan struct{}
	readers   chan struct{}
	ctx       context.Context
	cancel    context.CancelFunc
	wg        sync.WaitGroup
}

func Open(config Config, provider Provider) (*Server, error) {
	if len(config.Key) < 32 || config.Concurrency < 1 || config.Concurrency > 64 || len(config.CallbackOrigins) == 0 {
		return nil, errors.New("worker needs a retained signing key, bounded concurrency and admitted callback origins")
	}
	for _, origin := range config.CallbackOrigins {
		u, err := url.Parse(origin)
		if err != nil || u.User != nil || u.Host == "" || u.Path != "" || u.RawQuery != "" || u.Fragment != "" ||
			(u.Scheme != "https" && !(config.Development && u.Scheme == "http" && (u.Hostname() == "127.0.0.1" || u.Hostname() == "localhost"))) {
			return nil, errors.New("worker callback origins must be HTTPS or explicit loopback development HTTP")
		}
	}
	if err := os.MkdirAll(config.Directory, 0700); err != nil {
		return nil, err
	}
	lockDB, err := sql.Open("sqlite", filepath.Join(config.Directory, "writer.sqlite")+"?_txlock=immediate")
	if err != nil {
		return nil, err
	}
	lockDB.SetMaxOpenConns(1)
	if _, err = lockDB.Exec("CREATE TABLE IF NOT EXISTS writer(id INTEGER PRIMARY KEY)"); err != nil {
		lockDB.Close()
		return nil, err
	}
	lock, err := lockDB.BeginTx(context.Background(), nil)
	if err != nil {
		lockDB.Close()
		return nil, errors.New("worker receipt directory already has a live writer")
	}
	db, err := sql.Open("sqlite", filepath.Join(config.Directory, "receipts.sqlite")+"?_pragma=journal_mode(WAL)&_pragma=synchronous(FULL)&_pragma=busy_timeout(5000)&_pragma=cache_size(-2048)&_pragma=wal_autocheckpoint(128)&_txlock=immediate")
	if err != nil {
		lock.Rollback()
		lockDB.Close()
		return nil, err
	}
	db.SetMaxOpenConns(1)
	fail := func(err error) (*Server, error) {
		_ = db.Close()
		_ = lock.Rollback()
		_ = lockDB.Close()
		return nil, err
	}
	_, err = db.Exec(`CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,digest TEXT NOT NULL,payload BLOB NOT NULL,receipt BLOB,
outcome TEXT NOT NULL DEFAULT '',acknowledged INTEGER NOT NULL DEFAULT 0,created_ms INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS pending_receipts ON jobs(acknowledged,created_ms,id);`)
	if err != nil {
		return fail(err)
	}
	sum := sha256.Sum256(config.Key)
	keyID := hex.EncodeToString(sum[:])
	if _, err = db.Exec("INSERT INTO meta VALUES('key',?) ON CONFLICT DO NOTHING", keyID); err != nil {
		return fail(err)
	}
	var oldKey string
	if err = db.QueryRow("SELECT value FROM meta WHERE key='key'").Scan(&oldKey); err != nil {
		return fail(err)
	}
	if subtle.ConstantTimeCompare([]byte(oldKey), []byte(keyID)) != 1 {
		return fail(errors.New("worker retained receipts require their original custody key"))
	}
	if err = os.Chmod(filepath.Join(config.Directory, "receipts.sqlite"), 0600); err != nil {
		return fail(err)
	}
	block, err := aes.NewCipher(sum[:])
	if err != nil {
		return fail(err)
	}
	sealer, err := cipher.NewGCM(block)
	if err != nil {
		return fail(err)
	}
	v, err := validation.New()
	if err != nil {
		return fail(err)
	}
	client := network.Client(5 * time.Second)
	if config.Development {
		client = &http.Client{Timeout: 5 * time.Second, Transport: &http.Transport{DisableKeepAlives: true}}
	}
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	ctx, cancel := context.WithCancel(context.Background())
	s := &Server{config: config, db: db, lockDB: lockDB, lock: lock, cipher: sealer, provider: provider, validator: v,
		client: client, slots: make(chan struct{}, config.Concurrency), readers: make(chan struct{}, max(4, config.Concurrency*2)), ctx: ctx, cancel: cancel}
	if err = s.recoverClaims(); err != nil {
		cancel()
		return fail(err)
	}
	s.wg.Add(1)
	go s.reporter()
	return s, nil
}

func (s *Server) Close() error {
	s.cancel()
	s.wg.Wait()
	return errors.Join(s.db.Close(), s.lock.Rollback(), s.lockDB.Close())
}

func (s *Server) seal(id string, value any) ([]byte, error) {
	nonce := make([]byte, s.cipher.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, err
	}
	return s.cipher.Seal(nonce, nonce, kernel.JSON(value), []byte(id)), nil
}

func (s *Server) unseal(id string, data []byte, value any) error {
	size := s.cipher.NonceSize()
	if len(data) < size {
		return errors.New("worker evidence is truncated")
	}
	body, err := s.cipher.Open(nil, data[:size], data[size:], []byte(id))
	if err != nil {
		return errors.New("worker evidence failed custody authentication")
	}
	return json.Unmarshal(body, value)
}

func (s *Server) callbackAllowed(invocation wire.ManagedInvocation) bool {
	u, err := url.Parse(invocation.CallbackUrl)
	if err != nil || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return false
	}
	path := "/v2/sessions/" + invocation.Execution.Operation.SessionID + "/operations/" + invocation.Execution.Operation.ID + "/receipt"
	if u.Path != path || u.RawPath != "" {
		return false
	}
	for _, origin := range s.config.CallbackOrigins {
		if u.Scheme+"://"+u.Host == origin {
			return true
		}
	}
	return false
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	if r.Method == http.MethodGet && r.URL.Path == "/health/live" {
		w.WriteHeader(http.StatusNoContent)
		return
	}
	if r.Method != http.MethodPost || r.URL.Path != "/invoke" {
		http.NotFound(w, r)
		return
	}
	select {
	case s.readers <- struct{}{}:
		defer func() { <-s.readers }()
	default:
		http.Error(w, "worker request capacity exhausted", http.StatusTooManyRequests)
		return
	}
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 1<<20))
	if err != nil {
		http.Error(w, "invocation exceeds worker bound", http.StatusRequestEntityTooLarge)
		return
	}
	mac := hmac.New(sha256.New, s.config.Key)
	_, _ = mac.Write(body)
	expected := "v1=" + hex.EncodeToString(mac.Sum(nil))
	if !hmac.Equal([]byte(expected), []byte(r.Header.Get("X-Brain-Signature"))) {
		http.Error(w, "worker signature is required", http.StatusUnauthorized)
		return
	}
	if err = s.validator.Wire("ManagedInvocation", body); err != nil {
		http.Error(w, "invalid managed invocation", http.StatusBadRequest)
		return
	}
	var invocation wire.ManagedInvocation
	if json.Unmarshal(body, &invocation) != nil || !s.callbackAllowed(invocation) ||
		!kernel.Identifier(invocation.Execution.Operation.ID) || !kernel.Identifier(invocation.Execution.Operation.SessionID) ||
		invocation.Execution.Operation.Status != wire.OperationStatusDispatched ||
		invocation.Execution.Operation.ID != r.Header.Get("X-Brain-Operation") {
		http.Error(w, "invocation authority does not match its callback", http.StatusForbidden)
		return
	}
	if invocation.Execution.MaxOutputBytes == 0 || invocation.Execution.MaxOutputBytes > 196608 ||
		invocation.Execution.DeadlineMs > uint64(time.Now().Add(5*time.Minute).UnixMilli()) {
		http.Error(w, "worker output or deadline exceeds admission", http.StatusBadRequest)
		return
	}
	id, digest := invocation.Execution.Operation.ID, kernel.Digest(body)
	receipt, exists, err := s.existing(id, digest)
	if err != nil {
		http.Error(w, "worker claim conflicts with retained evidence", http.StatusConflict)
		return
	}
	if exists {
		s.write(w, receipt)
		return
	}
	if invocation.Execution.DeadlineMs <= uint64(time.Now().UnixMilli()) {
		s.write(w, wire.Receipt{Outcome: wire.OutcomeFailed, Error: kernel.Ptr("worker invocation expired before claim")})
		return
	}
	select {
	case s.slots <- struct{}{}:
	default:
		s.write(w, wire.Receipt{Outcome: wire.OutcomeFailed, Error: kernel.Ptr("worker capacity rejected this invocation before execution")})
		return
	}
	retained := invocation
	retained.Secrets = map[string]string{}
	encrypted, err := s.seal(id, retained)
	if err != nil {
		<-s.slots
		http.Error(w, "worker custody unavailable", http.StatusServiceUnavailable)
		return
	}
	result, err := s.db.ExecContext(r.Context(), `INSERT INTO jobs(id,digest,payload,created_ms)
SELECT ?,?,?,? WHERE (SELECT COUNT(*) FROM jobs WHERE acknowledged=0)<256 AND (SELECT COUNT(*) FROM jobs)<1000000`,
		id, digest, encrypted, time.Now().UnixMilli())
	inserted := int64(0)
	if err == nil {
		inserted, err = result.RowsAffected()
	}
	if err != nil || inserted != 1 {
		<-s.slots
		http.Error(w, "worker durable claim unavailable; execution did not receive permission", http.StatusServiceUnavailable)
		return
	}
	done := make(chan wire.Receipt, 1)
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		defer func() { <-s.slots }()
		ctx, cancel := context.WithDeadline(s.ctx, time.UnixMilli(int64(invocation.Execution.DeadlineMs)))
		defer cancel()
		receipt := s.provider.Run(ctx, invocation)
		receipt.RequestID = "worker:" + id
		if receipt.Outcome != wire.OutcomeCompleted && receipt.Outcome != wire.OutcomeFailed && receipt.Outcome != wire.OutcomeUnknown {
			receipt = wire.Receipt{RequestID: "worker:" + id, Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("worker returned no recognized outcome")}
		}
		if uint64(len(receipt.Result)) > invocation.Execution.MaxOutputBytes || len(kernel.JSON(receipt)) > 262144 {
			receipt.Result, receipt.Error = nil, kernel.Ptr("worker receipt bytes exceeded admission; the outcome is retained")
		}
		if err := s.store(id, receipt); err != nil {
			done <- wire.Receipt{RequestID: "worker-uncertain:" + id, Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("worker could not durably retain its completion")}
			return
		}
		done <- receipt
	}()
	select {
	case receipt := <-done:
		s.write(w, receipt)
	case <-r.Context().Done():
	case <-s.ctx.Done():
	}
}

func (s *Server) write(w http.ResponseWriter, receipt wire.Receipt) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(receipt)
}

func (s *Server) existing(id, digest string) (wire.Receipt, bool, error) {
	var storedDigest, outcome string
	var encrypted []byte
	err := s.db.QueryRow("SELECT digest,outcome,receipt FROM jobs WHERE id=?", id).Scan(&storedDigest, &outcome, &encrypted)
	if errors.Is(err, sql.ErrNoRows) {
		return wire.Receipt{}, false, nil
	}
	if err != nil || storedDigest != digest {
		return wire.Receipt{}, false, errors.New("worker claim digest conflict")
	}
	receipt := wire.Receipt{RequestID: "worker-uncertain:" + id, Outcome: wire.OutcomeUnknown,
		Error: kernel.Ptr("worker claim already exists; this request cannot execute it again")}
	if len(encrypted) > 0 {
		if err = s.unseal(id, encrypted, &receipt); err != nil {
			return receipt, true, err
		}
	} else if outcome != "" {
		receipt.Outcome = wire.Outcome(outcome)
		receipt.Error = kernel.Ptr("worker receipt was acknowledged; obtain retained result bytes from Brain")
	}
	return receipt, true, nil
}

func (s *Server) store(id string, receipt wire.Receipt) error {
	encrypted, err := s.seal(id, receipt)
	if err != nil {
		return err
	}
	result, err := s.db.Exec("UPDATE jobs SET receipt=?,outcome=? WHERE id=? AND receipt IS NULL AND acknowledged=0", encrypted, receipt.Outcome, id)
	if err != nil {
		return err
	}
	count, err := result.RowsAffected()
	if err != nil || count != 1 {
		return errors.New("worker outcome could not be durably settled")
	}
	return nil
}

func (s *Server) recoverClaims() error {
	rows, err := s.db.Query("SELECT id FROM jobs WHERE receipt IS NULL AND acknowledged=0 LIMIT 257")
	if err != nil {
		return err
	}
	ids := []string{}
	for rows.Next() {
		var id string
		if err = rows.Scan(&id); err != nil {
			rows.Close()
			return err
		}
		ids = append(ids, id)
	}
	err = rows.Err()
	rows.Close()
	if err != nil || len(ids) > 256 {
		return errors.New("worker pending claims exceed their retained bound")
	}
	for _, id := range ids {
		receipt := wire.Receipt{RequestID: "worker-recovery:" + id, Outcome: wire.OutcomeUnknown,
			Error: kernel.Ptr("worker was lost after durable claim; supplied code is not automatically reexecuted")}
		if err := s.store(id, receipt); err != nil {
			return err
		}
	}
	return nil
}

func (s *Server) reporter() {
	defer s.wg.Done()
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-s.ctx.Done():
			return
		case <-ticker.C:
			_ = s.report()
		}
	}
}

func (s *Server) report() error {
	rows, err := s.db.QueryContext(s.ctx, "SELECT id,payload,receipt FROM jobs WHERE acknowledged=0 AND receipt IS NOT NULL ORDER BY created_ms,id LIMIT 16")
	if err != nil {
		return err
	}
	type pending struct {
		id               string
		payload, receipt []byte
	}
	items := []pending{}
	for rows.Next() {
		var item pending
		if err = rows.Scan(&item.id, &item.payload, &item.receipt); err != nil {
			rows.Close()
			return err
		}
		items = append(items, item)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	for _, item := range items {
		var invocation wire.ManagedInvocation
		var receipt wire.Receipt
		if err = s.unseal(item.id, item.payload, &invocation); err != nil {
			return err
		}
		if err = s.unseal(item.id, item.receipt, &receipt); err != nil {
			return err
		}
		request, err := http.NewRequestWithContext(s.ctx, http.MethodPost, invocation.CallbackUrl, bytes.NewReader(kernel.JSON(receipt)))
		if err != nil || !s.callbackAllowed(invocation) {
			return errors.New("retained worker callback authority is invalid")
		}
		request.GetBody = nil
		request.Header.Set("Authorization", "Bearer "+invocation.Execution.ReceiptToken)
		request.Header.Set("Content-Type", "application/json")
		response, err := s.client.Do(request)
		if err != nil {
			continue
		}
		_, readErr := io.Copy(io.Discard, io.LimitReader(response.Body, 1<<20))
		response.Body.Close()
		if response.StatusCode >= 200 && response.StatusCode < 300 && readErr == nil {
			_, err = s.db.ExecContext(s.ctx, "UPDATE jobs SET acknowledged=1,payload=X'',receipt=NULL WHERE id=? AND acknowledged=0", item.id)
			if err != nil {
				return err
			}
		}
	}
	return nil
}

func (s *Server) String() string {
	return fmt.Sprintf("managed worker (capacity %d; callbacks %s)", s.config.Concurrency, strings.Join(s.config.CallbackOrigins, ","))
}
