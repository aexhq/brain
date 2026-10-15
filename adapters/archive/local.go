package archive

import (
	"context"
	"crypto/rand"
	"database/sql"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	_ "modernc.org/sqlite"
)

type Local struct {
	db       *sql.DB
	identity string
}

func OpenLocal(directory string) (*Local, error) {
	if err := os.MkdirAll(directory, 0700); err != nil {
		return nil, err
	}
	location := filepath.Join(directory, "objects.sqlite")
	if _, err := os.Stat(location); errors.Is(err, fs.ErrNotExist) {
		entries, e := os.ReadDir(directory)
		if e != nil {
			return nil, e
		}
		if len(entries) > 0 {
			return nil, kernel.Fail(wire.ErrorCodeUnsupported, "unrecognized local archive layout; preserve it and use a stopped-writer migration")
		}
	}
	db, err := sql.Open("sqlite", location+"?_pragma=journal_mode(WAL)&_pragma=synchronous(FULL)&_pragma=busy_timeout(5000)&_pragma=cache_size(-8192)&_pragma=mmap_size(0)&_pragma=wal_autocheckpoint(256)&_pragma=journal_size_limit(16777216)&_txlock=immediate")
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	_, err = db.Exec(`CREATE TABLE IF NOT EXISTS objects(key TEXT PRIMARY KEY,digest TEXT NOT NULL,size INTEGER NOT NULL,body BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS archive_format(version INTEGER PRIMARY KEY);
INSERT INTO archive_format VALUES(1) ON CONFLICT DO NOTHING;`)
	if err != nil {
		db.Close()
		return nil, err
	}
	var version int
	if err = db.QueryRow("SELECT MAX(version) FROM archive_format").Scan(&version); err != nil || version != 1 {
		db.Close()
		return nil, kernel.Fail(wire.ErrorCodeUnsupported, "unsupported local immutable archive format")
	}
	if _, err = db.Exec("CREATE TABLE IF NOT EXISTS archive_identity(id INTEGER PRIMARY KEY CHECK(id=1),value TEXT NOT NULL)"); err != nil {
		db.Close()
		return nil, err
	}
	if _, err = db.Exec("INSERT INTO archive_identity VALUES(1,?) ON CONFLICT DO NOTHING", kernel.ID("archive", rand.Text())); err != nil {
		db.Close()
		return nil, err
	}
	var identity string
	if err = db.QueryRow("SELECT value FROM archive_identity WHERE id=1").Scan(&identity); err != nil {
		db.Close()
		return nil, err
	}
	return &Local{db: db, identity: identity}, nil
}

func (s *Local) Close() error     { return s.db.Close() }
func (s *Local) Identity() string { return "local:" + s.identity }
func validKey(key string) bool {
	return fs.ValidPath(key) && !strings.ContainsAny(key, "\\:") && key != "." && len(key) <= 1024
}

func (s *Local) Put(ctx context.Context, key string, body []byte) error {
	if !validKey(key) || len(body) > DynamoMaximum {
		return kernel.Fail(wire.ErrorCodeInvalid, "invalid immutable object key or size")
	}
	digest := kernel.Digest(body)
	result, err := s.db.ExecContext(ctx, "INSERT INTO objects(key,digest,size,body) VALUES(?,?,?,?) ON CONFLICT DO NOTHING", key, digest, len(body), body)
	if err != nil {
		return err
	}
	count, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if count == 0 {
		prior, err := s.Get(ctx, key, int64(len(body)))
		if err != nil {
			return err
		}
		if kernel.Digest(prior) != digest {
			return kernel.Fail(wire.ErrorCodeCorrupt, "immutable local object conflict")
		}
	}
	return nil
}

func (s *Local) Get(ctx context.Context, key string, limit int64) ([]byte, error) {
	if !validKey(key) || limit < 0 || limit > DynamoMaximum {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid immutable object read")
	}
	var body []byte
	var digest string
	err := s.db.QueryRowContext(ctx, "SELECT digest,body FROM objects WHERE key=? AND size<=? AND LENGTH(body)<=?", key, limit, limit).Scan(&digest, &body)
	if errors.Is(err, sql.ErrNoRows) {
		var exists int
		if err = s.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM objects WHERE key=?", key).Scan(&exists); err != nil {
			return nil, err
		}
		if exists == 0 {
			return nil, kernel.Fail(wire.ErrorCodeUnavailable, "archive object is missing")
		}
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "archive object exceeds its declared limit")
	}
	if err != nil {
		return nil, err
	}
	if kernel.Digest(body) != digest {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "local archive digest mismatch")
	}
	return body, nil
}

func (s *Local) Delete(ctx context.Context, key string) error {
	if !validKey(key) {
		return kernel.Fail(wire.ErrorCodeInvalid, "invalid immutable object key")
	}
	_, err := s.db.ExecContext(ctx, "DELETE FROM objects WHERE key=?", key)
	return err
}
