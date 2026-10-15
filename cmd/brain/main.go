package main

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/aexhq/brain/adapters/archive"
	"github.com/aexhq/brain/adapters/d1control"
	"github.com/aexhq/brain/adapters/harness"
	"github.com/aexhq/brain/adapters/integrations"
	"github.com/aexhq/brain/adapters/models"
	"github.com/aexhq/brain/adapters/sqlcontrol"
	"github.com/aexhq/brain/adapters/storage"
	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/history"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/server"
)

var version = "development"

func main() {
	if len(os.Args) > 1 && os.Args[1] == "version" {
		fmt.Println("Brain", version, "brain-session/2")
		return
	}
	if err := run(); err != nil {
		slog.Error("startup_failed", "message", err.Error())
		os.Exit(1)
	}
}

func env(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}
func number(name string, fallback int64) (int64, error) {
	if value := os.Getenv(name); value != "" {
		n, err := strconv.ParseInt(value, 10, 64)
		if err != nil || n < 0 {
			return 0, fmt.Errorf("%s must be a non-negative integer", name)
		}
		return n, nil
	}
	return fallback, nil
}

func initialize(directory string, supplied []byte) ([]byte, error) {
	if err := os.MkdirAll(directory, 0700); err != nil {
		return nil, err
	}
	marker := filepath.Join(directory, "format")
	body, err := os.ReadFile(marker)
	if errors.Is(err, os.ErrNotExist) {
		entries, e := os.ReadDir(directory)
		if e != nil {
			return nil, e
		}
		if len(entries) > 0 {
			return nil, fmt.Errorf("nonempty data directory has no brain-data/4 marker; preserve its matching runtime and use the stopped-writer compatibility procedure")
		}
		file, e := os.OpenFile(marker, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if e != nil {
			return nil, e
		}
		_, e = file.WriteString("brain-data/4\n")
		if e == nil {
			e = file.Sync()
		}
		e = errors.Join(e, file.Close())
		if e != nil {
			return nil, e
		}
	} else if err != nil {
		return nil, err
	} else if string(body) != "brain-data/4\n" {
		return nil, fmt.Errorf("data format is not brain-data/4; no marker rewrite or empty-volume substitution is permitted")
	}
	keyPath := filepath.Join(directory, "custody.key")
	key, err := os.ReadFile(keyPath)
	if errors.Is(err, os.ErrNotExist) {
		entries, e := os.ReadDir(directory)
		if e != nil {
			return nil, e
		}
		if len(entries) != 1 {
			return nil, fmt.Errorf("retained custody key is missing; do not regenerate it")
		}
		key = supplied
		if len(key) == 0 {
			key = make([]byte, 32)
			if _, e = rand.Read(key); e != nil {
				return nil, e
			}
		}
		file, e := os.OpenFile(keyPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if e != nil {
			return nil, e
		}
		_, e = file.Write(key)
		if e == nil {
			e = file.Sync()
		}
		e = errors.Join(e, file.Close())
		if e != nil {
			return nil, e
		}
	} else if err != nil {
		return nil, err
	}
	if len(key) != 32 {
		return nil, fmt.Errorf("invalid retained custody key")
	}
	if len(supplied) > 0 && subtle.ConstantTimeCompare(supplied, key) != 1 {
		return nil, fmt.Errorf("configured custody key differs from retained data; the original key cannot be replaced")
	}
	return key, nil
}

func run() error {
	data := env("BRAIN_DATA", ".brain")
	listen := env("BRAIN_LISTEN", "127.0.0.1:8080")
	development := os.Getenv("BRAIN_DEVELOPMENT") == "1"
	token := os.Getenv("BRAIN_API_TOKEN")
	host, _, err := net.SplitHostPort(listen)
	if err != nil {
		return fmt.Errorf("invalid BRAIN_LISTEN")
	}
	if token == "" && (!development || (host != "127.0.0.1" && host != "::1" && host != "localhost")) {
		return fmt.Errorf("BRAIN_API_TOKEN is required except in explicit loopback development mode")
	}
	var configuredKey []byte
	if raw := os.Getenv("BRAIN_CUSTODY_KEY"); raw != "" {
		configuredKey, err = hex.DecodeString(raw)
		if err != nil || len(configuredKey) != 32 {
			return fmt.Errorf("BRAIN_CUSTODY_KEY must contain exactly 32 bytes in hexadecimal")
		}
	}
	key, err := initialize(data, configuredKey)
	if err != nil {
		return err
	}
	nodeBytes := make([]byte, 16)
	if _, err = rand.Read(nodeBytes); err != nil {
		return err
	}
	node := env("BRAIN_NODE_NAME", "node") + "-" + hex.EncodeToString(nodeBytes)
	ctx := context.Background()
	var authority control.Store
	switch env("BRAIN_CONTROL", "sql") {
	case "sql":
		authority, err = sqlcontrol.Open(ctx, env("BRAIN_CONTROL_URL", filepath.Join(data, "control.sqlite")), development)
	case "d1":
		signing, e := hex.DecodeString(os.Getenv("BRAIN_D1_KEY"))
		if e != nil || len(signing) != 32 {
			return fmt.Errorf("BRAIN_D1_KEY must contain exactly 32 bytes in hexadecimal")
		}
		authority, err = d1control.Open(ctx, os.Getenv("BRAIN_CONTROL_URL"), signing, development)
	default:
		return fmt.Errorf("unsupported BRAIN_CONTROL")
	}
	if err != nil {
		return err
	}
	defer authority.Close()
	var blobs history.Archive
	archiveSettings := archive.AWSConfig{Region: env("BRAIN_ARCHIVE_REGION", "auto"), Endpoint: os.Getenv("BRAIN_ARCHIVE_ENDPOINT"),
		AccessKey: os.Getenv("BRAIN_ARCHIVE_ACCESS_KEY_ID"), SecretKey: os.Getenv("BRAIN_ARCHIVE_SECRET_ACCESS_KEY"),
		Bucket: os.Getenv("BRAIN_ARCHIVE_BUCKET"), Table: os.Getenv("BRAIN_ARCHIVE_TABLE"), Prefix: env("BRAIN_ARCHIVE_PREFIX", "brain-internal/"),
		PathStyle: os.Getenv("BRAIN_ARCHIVE_PATH_STYLE") == "1"}
	switch env("BRAIN_ARCHIVE", "local") {
	case "local":
		local, e := archive.OpenLocal(env("BRAIN_ARCHIVE_DIRECTORY", filepath.Join(data, "archive")))
		if e != nil {
			return e
		}
		defer local.Close()
		blobs = local
	case "s3", "r2":
		blobs, err = archive.OpenS3(ctx, archiveSettings)
	case "dynamodb":
		blobs, err = archive.OpenDynamo(ctx, archiveSettings)
	default:
		return fmt.Errorf("unsupported BRAIN_ARCHIVE")
	}
	if err != nil {
		return err
	}
	cache, err := number("BRAIN_L1_BYTES", 64<<20)
	if err != nil {
		return err
	}
	disk, err := number("BRAIN_L2_BYTES", 2<<30)
	if err != nil {
		return err
	}
	dirty, err := number("BRAIN_DIRTY_BYTES", 512<<20)
	if err != nil {
		return err
	}
	shards, err := number("BRAIN_L2_SHARDS", 4)
	if err != nil {
		return err
	}
	workers, err := number("BRAIN_WORKERS", 64)
	if err != nil {
		return err
	}
	inflight, err := number("BRAIN_INFLIGHT", 256)
	if err != nil {
		return err
	}
	requests, err := number("BRAIN_REQUESTS", 128)
	if err != nil {
		return err
	}
	files, err := number("BRAIN_FILE_TRANSFERS", 4)
	if err != nil {
		return err
	}
	streams, err := number("BRAIN_STREAMS", 1024)
	if err != nil {
		return err
	}
	lease, err := number("BRAIN_LEASE_MS", 15000)
	if err != nil {
		return err
	}
	archiveInterval, err := number("BRAIN_ARCHIVE_FLUSH_MS", 1000)
	if err != nil {
		return err
	}
	records, err := history.Open(ctx, history.Config{Directory: filepath.Join(data, "history"), Shards: int(shards), CacheBytes: cache, DiskBytes: disk, DirtyBytes: dirty, Subscribers: int(streams)}, authority, blobs, node)
	if err != nil {
		return err
	}
	defer records.Close()
	var registrations []harness.Registration
	if err = json.Unmarshal([]byte(env("BRAIN_HARNESSES", "[]")), &registrations); err != nil {
		return fmt.Errorf("invalid BRAIN_HARNESSES")
	}
	loops, err := harness.New(registrations)
	if err != nil {
		return err
	}
	providers := map[string]models.Provider{}
	if err = json.Unmarshal([]byte(env("BRAIN_MODELS", `{"openai":{"kind":"openai-responses","credential":"openai"},"anthropic":{"kind":"anthropic","credential":"anthropic"}}`)), &providers); err != nil {
		return fmt.Errorf("invalid BRAIN_MODELS")
	}
	secrets := map[string]map[string]string{"local": {"openai": "OPENAI_API_KEY", "anthropic": "ANTHROPIC_API_KEY"}}
	if raw := os.Getenv("BRAIN_SECRET_REFS"); raw != "" {
		if err = json.Unmarshal([]byte(raw), &secrets); err != nil {
			return fmt.Errorf("invalid BRAIN_SECRET_REFS")
		}
	}
	resolve := func(ctx context.Context, tenant, name string) (string, error) {
		variable := secrets[tenant][name]
		if variable == "" || os.Getenv(variable) == "" {
			return "", kernel.Fail(wire.ErrorCodeForbidden, "scoped secret is not configured")
		}
		return os.Getenv(variable), nil
	}
	modelRegistry := models.New(providers, resolve)
	effects := &integrations.Registry{Models: modelRegistry, Handlers: map[string]integrations.Handler{
		"http":      &integrations.HTTP{Resolve: resolve, Development: development},
		"web_fetch": &integrations.Fetch{},
		"brave":     &integrations.Search{Provider: "brave", Resolve: resolve},
		"tavily":    &integrations.Search{Provider: "tavily", Resolve: resolve},
		"exa":       &integrations.Search{Provider: "exa", Resolve: resolve},
		"mcp":       &integrations.MCP{Resolve: resolve, Development: development},
	}, Entries: []wire.CatalogEntry{}}
	for tenant, refs := range secrets {
		for ref, variable := range refs {
			if os.Getenv(variable) != "" {
				effects.Secrets = append(effects.Secrets, kernel.Capability("secret", tenant, ref))
			}
		}
	}
	agentLocal, err := storage.OpenLocal(filepath.Join(data, "agent-storage"), authority, blobs)
	if err != nil {
		return err
	}
	defer agentLocal.Close()
	effects.Handlers["local_storage"] = agentLocal
	for _, name := range []string{"s3", "r2"} {
		prefix := "BRAIN_AGENT_" + strings.ToUpper(name) + "_"
		bucket := os.Getenv(prefix + "BUCKET")
		if bucket == "" {
			continue
		}
		if bucket == archiveSettings.Bucket {
			return fmt.Errorf("agent-facing storage must use a different bucket and credentials from transcript storage")
		}
		settings := archive.AWSConfig{Bucket: bucket, Region: os.Getenv(prefix + "REGION"), Endpoint: os.Getenv(prefix + "ENDPOINT"),
			AccessKey: os.Getenv(prefix + "ACCESS_KEY_ID"), SecretKey: os.Getenv(prefix + "SECRET_ACCESS_KEY"), Prefix: "agent-data", PathStyle: os.Getenv(prefix+"PATH_STYLE") == "1"}
		if settings.AccessKey == "" || settings.SecretKey == "" {
			return fmt.Errorf("agent-facing object storage requires its own explicit credentials")
		}
		if settings.AccessKey == archiveSettings.AccessKey && settings.SecretKey == archiveSettings.SecretKey {
			return fmt.Errorf("agent-facing storage cannot reuse transcript credentials")
		}
		objects, e := archive.OpenS3(ctx, settings)
		if e != nil {
			return e
		}
		effects.Handlers[name+"_storage"] = &storage.Handler{Object: objects, Control: authority, Blobs: blobs}
	}
	computeWorkers := map[string]struct {
		URL     string `json:"url"`
		Signing string `json:"signing"`
	}{}
	if err = json.Unmarshal([]byte(env("BRAIN_COMPUTE_WORKERS", "{}")), &computeWorkers); err != nil {
		return fmt.Errorf("invalid BRAIN_COMPUTE_WORKERS")
	}
	for name, worker := range computeWorkers {
		if name != "local_compute" {
			return fmt.Errorf("compute worker provider is not installed")
		}
		handler := &integrations.Managed{Provider: name, URL: worker.URL, Signing: worker.Signing, PublicURL: os.Getenv("BRAIN_PUBLIC_URL"),
			Control: authority, Resolve: resolve, Development: development}
		if err = handler.Validate(wire.ConnectionTemplate{Provider: name, Kind: wire.ConnectionKindLocal, Ownership: wire.OwnershipManaged}); err != nil {
			return err
		}
		effects.Handlers[name] = handler
	}
	listener, err := net.Listen("tcp", listen)
	if err != nil {
		return err
	}
	defer listener.Close()
	internalURL := os.Getenv("BRAIN_INTERNAL_URL")
	if internalURL == "" && development && (host == "127.0.0.1" || host == "localhost" || host == "::1") {
		internalURL = "http://" + listener.Addr().String()
	}
	app, err := server.New(server.Config{Node: node, Token: token, Key: key, Workers: int(workers), Inflight: int(inflight),
		Requests: int(requests), FileTransfers: int(files), Streams: int(streams),
		Lease: time.Duration(lease) * time.Millisecond, Development: development, Origins: strings.Fields(os.Getenv("BRAIN_BROWSER_ORIGINS")), InternalURL: internalURL, ArchiveInterval: time.Duration(archiveInterval) * time.Millisecond},
		authority, records, blobs, loops, effects)
	if err != nil {
		return err
	}
	if err = app.Start(ctx); err != nil {
		return err
	}
	httpServer := &http.Server{Addr: listen, Handler: app, ReadHeaderTimeout: 10 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 16384}
	stopped := make(chan error, 1)
	go func() { stopped <- httpServer.Serve(listener) }()
	slog.Info("ready", "listen", listener.Addr().String(), "node", node, "format", "brain-data/4")
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt, syscall.SIGTERM)
	defer signal.Stop(signals)
	select {
	case <-signals:
	case err = <-stopped:
		if err != nil && !errors.Is(err, http.ErrServerClosed) {
			return err
		}
	}
	stop, cancel := context.WithTimeout(context.Background(), 440*time.Second)
	defer cancel()
	shutdown := make(chan error, 1)
	go func() { shutdown <- httpServer.Shutdown(stop) }()
	err = app.Stop(stop)
	return errors.Join(err, <-shutdown)
}
