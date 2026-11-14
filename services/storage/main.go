package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"io"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aexhq/brain/services/storage/internal/adapters/dynamodb"
	"github.com/aexhq/brain/services/storage/internal/adapters/filesystem"
	"github.com/aexhq/brain/services/storage/internal/adapters/postgres"
	"github.com/aexhq/brain/services/storage/internal/adapters/s3"
	"github.com/aexhq/brain/services/storage/internal/adapters/sqlite"
	"github.com/aexhq/brain/services/storage/internal/server"
	"github.com/aexhq/brain/services/storage/internal/sessions/metadata"
	"github.com/aexhq/brain/services/storage/internal/tiering"
	"github.com/aexhq/brain/telemetry"
)

type config struct {
	Listen           string          `json:"listen"`
	DataDir          string          `json:"data_dir"`
	TokenEnv         string          `json:"token_env"`
	MemoryBytes      int64           `json:"memory_bytes"`
	DiskBytes        int64           `json:"disk_bytes"`
	MemoryTTLSeconds int             `json:"memory_ttl_seconds"`
	DiskTTLSeconds   int             `json:"disk_ttl_seconds"`
	SyncSeconds      int             `json:"sync_seconds"`
	Objects          adapters.Config `json:"objects"`
	Metadata         adapters.Config `json:"metadata"`
	MetadataBytes    int64           `json:"metadata_disk_bytes"`
}

func backend(ctx context.Context, c adapters.Config) (adapters.Adapter, error) {
	secret := func(name string) (string, error) {
		if name == "" {
			return "", nil
		}
		v, ok := os.LookupEnv(name)
		if !ok || v == "" {
			return "", errors.New("configured storage credential environment is empty")
		}
		return v, nil
	}
	a, e := secret(c.AccessKeyEnv)
	if e != nil {
		return nil, e
	}
	b, e := secret(c.SecretKeyEnv)
	if e != nil {
		return nil, e
	}
	switch c.Kind {
	case "filesystem":
		if c.Path == "" {
			return nil, errors.New("filesystem path is required")
		}
		return filesystem.New(c.Path)
	case "sqlite":
		if c.Path == "" {
			return nil, errors.New("SQLite path is required")
		}
		return sqlite.New(c.Path)
	case "s3", "r2":
		return s3.New(ctx, s3.Config{Bucket: c.Bucket, Region: c.Region, Endpoint: c.Endpoint, Prefix: c.Prefix, PathStyle: c.PathStyle, AccessKey: a, SecretKey: b})
	case "dynamodb":
		return dynamodb.New(ctx, dynamodb.Config{Table: c.Table, Region: c.Region, Endpoint: c.Endpoint, AccessKey: a, SecretKey: b})
	case "postgres", "supabase":
		dsn, e := secret(c.DSNEnv)
		if e != nil {
			return nil, e
		}
		return postgres.New(ctx, dsn)
	default:
		return nil, errors.New("unknown external storage backend")
	}
}
func main() {
	if err := run(); err != nil {
		slog.Error("storage stopped", "error", err)
		os.Exit(1)
	}
}
func run() error {
	file := flag.String("config", "storage.json", "storage configuration")
	flag.Parse()
	f, e := os.Open(*file)
	if e != nil {
		return e
	}
	defer f.Close()
	c := config{Listen: ":8081", TokenEnv: "BRAIN_SERVICE_TOKEN", MemoryBytes: 128 << 20, DiskBytes: 4 << 30, MetadataBytes: 64 << 20, MemoryTTLSeconds: 300, DiskTTLSeconds: 86400, SyncSeconds: 5}
	d := json.NewDecoder(io.LimitReader(f, 1<<20))
	d.DisallowUnknownFields()
	if e = d.Decode(&c); e != nil {
		return e
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing configuration data")
	}
	if c.DataDir == "" || c.SyncSeconds <= 0 {
		return errors.New("data directory and positive synchronization interval are required")
	}
	if e = os.MkdirAll(c.DataDir, 0700); e != nil {
		return e
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	_, shutdown, e := telemetry.SetupService(ctx, "storage")
	if e != nil {
		return e
	}
	defer func() {
		c, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = shutdown(c)
	}()
	remote, e := backend(ctx, c.Objects)
	if e != nil {
		return e
	}
	defer remote.Close()
	index, e := backend(ctx, c.Metadata)
	if e != nil {
		return e
	}
	defer index.Close()
	disk, e := filesystem.New(filepath.Join(c.DataDir, "objects"))
	if e != nil {
		return e
	}
	objects, e := tiering.New(ctx, tiering.Options{MinFreeBytes: 256 << 20, Disk: disk, Remote: remote, Index: filepath.Join(c.DataDir, "tiering.sqlite"), MemoryBytes: c.MemoryBytes, DiskBytes: c.DiskBytes, MemoryTTL: time.Duration(c.MemoryTTLSeconds) * time.Second, DiskTTL: time.Duration(c.DiskTTLSeconds) * time.Second, Interval: time.Second})
	if e != nil {
		return e
	}
	defer objects.Close()
	local, e := sqlite.New(filepath.Join(c.DataDir, "metadata.sqlite"))
	if e != nil {
		return e
	}
	metadataCache, e := tiering.New(ctx, tiering.Options{MinFreeBytes: 256 << 20, Disk: local, Index: filepath.Join(c.DataDir, "metadata-tiering.sqlite"), MemoryBytes: 0, DiskBytes: c.MetadataBytes, MemoryTTL: time.Second, DiskTTL: time.Duration(c.DiskTTLSeconds) * time.Second, Interval: time.Second})
	if e != nil {
		local.Close()
		return e
	}
	defer metadataCache.Close()
	records := &metadata.Store{Disk: metadataCache, Remote: index, Objects: objects}
	h, e := (&server.Server{Store: records, Token: os.Getenv(c.TokenEnv)}).Handler()
	if e != nil {
		return e
	}
	workerDone := make(chan struct{})
	go func() {
		defer close(workerDone)
		records.Replicate(ctx, time.Duration(c.SyncSeconds)*time.Second, func(e error) { slog.Error("replication failed", "error", e) })
	}()
	defer func() { stop(); <-workerDone }()
	s := &http.Server{Addr: c.Listen, Handler: h, ReadHeaderTimeout: 10 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 32 << 10}
	done := make(chan error, 1)
	go func() { done <- s.ListenAndServe() }()
	select {
	case e := <-done:
		if errors.Is(e, http.ErrServerClosed) {
			return nil
		}
		return e
	case <-ctx.Done():
	}
	deadline, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	return s.Shutdown(deadline)
}
