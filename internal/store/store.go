// Package store owns local metadata and append-only event files.
package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/aexhq/brain/internal/core"
	_ "modernc.org/sqlite"
)

type Store struct {
	db   *sql.DB
	root string
}

func Open(root string) (*Store, error) {
	root, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(root, 0700); err != nil {
		return nil, err
	}
	db, err := sql.Open("sqlite", filepath.Join(root, "metadata.sqlite"))
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	_, err = db.Exec(`PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;
CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, created TEXT NOT NULL, data BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS sessions_tenant ON sessions(tenant, id);`)
	if err != nil {
		db.Close()
		return nil, err
	}
	return &Store{db: db, root: root}, nil
}

func (s *Store) Close() error { return s.db.Close() }
func (s *Store) Root() string { return s.root }
func (s *Store) Dir(id string) (string, error) {
	if !core.ValidID(id) {
		return "", core.E(core.Invalid, "invalid session id")
	}
	return filepath.Join(s.root, "sessions", id), nil
}

func (s *Store) Create(ctx context.Context, session core.Session) error {
	data, err := json.Marshal(session)
	if err != nil {
		return err
	}
	dir, err := s.Dir(session.ID)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(dir, "native"), 0700); err != nil {
		return err
	}
	_, err = s.db.ExecContext(ctx, `INSERT INTO sessions(id,tenant,created,data) VALUES(?,?,?,?)`, session.ID, session.Tenant, session.CreatedAt.Format(time.RFC3339Nano), data)
	return err
}

func (s *Store) Get(ctx context.Context, id string) (core.Session, error) {
	var session core.Session
	var data []byte
	err := s.db.QueryRowContext(ctx, `SELECT tenant,data FROM sessions WHERE id=?`, id).Scan(&session.Tenant, &data)
	if errors.Is(err, sql.ErrNoRows) {
		return session, core.E(core.NotFound, "session not found")
	}
	if err != nil {
		return session, err
	}
	if err := json.Unmarshal(data, &session); err != nil {
		return session, fmt.Errorf("unreadable session metadata: %w", err)
	}
	return session, nil
}

func (s *Store) Save(ctx context.Context, session core.Session) error {
	data, err := json.Marshal(session)
	if err != nil {
		return err
	}
	r, err := s.db.ExecContext(ctx, `UPDATE sessions SET data=? WHERE id=? AND tenant=?`, data, session.ID, session.Tenant)
	if err != nil {
		return err
	}
	n, err := r.RowsAffected()
	if err == nil && n != 1 {
		return core.E(core.NotFound, "session not found")
	}
	return err
}

func (s *Store) List(ctx context.Context, tenant, after string, limit int) ([]core.Session, error) {
	if limit < 1 || limit > 1000 {
		return nil, core.E(core.Invalid, "invalid list limit")
	}
	rows, err := s.db.QueryContext(ctx, `SELECT tenant,data FROM sessions WHERE tenant=? AND id>? ORDER BY id LIMIT ?`, tenant, after, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := make([]core.Session, 0)
	for rows.Next() {
		var session core.Session
		var data []byte
		if err := rows.Scan(&session.Tenant, &data); err != nil {
			return nil, err
		}
		if err := json.Unmarshal(data, &session); err != nil {
			return nil, fmt.Errorf("unreadable session metadata: %w", err)
		}
		result = append(result, session)
	}
	return result, rows.Err()
}

// Scan is host-only discovery for bounded synchronization after restart.
func (s *Store) Scan(ctx context.Context, after string, limit int) ([]core.Session, error) {
	if limit < 1 || limit > 1000 {
		return nil, errors.New("invalid scan limit")
	}
	rows, err := s.db.QueryContext(ctx, `SELECT tenant,data FROM sessions WHERE id>? ORDER BY id LIMIT ?`, after, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := []core.Session{}
	for rows.Next() {
		var session core.Session
		var data []byte
		if err := rows.Scan(&session.Tenant, &data); err != nil {
			return nil, err
		}
		if err := json.Unmarshal(data, &session); err != nil {
			return nil, err
		}
		result = append(result, session)
	}
	return result, rows.Err()
}

// Import is called only after a complete staged directory has been installed.
func (s *Store) Import(ctx context.Context, session core.Session) error {
	data, err := json.Marshal(session)
	if err != nil {
		return err
	}
	r, err := s.db.ExecContext(ctx, `INSERT INTO sessions(id,tenant,created,data) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data WHERE sessions.tenant=excluded.tenant`, session.ID, session.Tenant, session.CreatedAt.Format(time.RFC3339Nano), data)
	if err != nil {
		return err
	}
	n, err := r.RowsAffected()
	if err == nil && n != 1 {
		return core.E(core.Forbidden, "session belongs to another tenant")
	}
	return err
}
