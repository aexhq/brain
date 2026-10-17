// Package store owns local metadata and append-only event files.
package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/aexhq/brain/internal/core"
	_ "modernc.org/sqlite"
)

// SQLite implements the authoritative local metadata database.
type SQLite struct{ db *sql.DB }

func OpenSQLite(file string) (*SQLite, error) {
	db, err := sql.Open("sqlite", file)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	_, err = db.Exec(`PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;
CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, created TEXT NOT NULL, data BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS sessions_tenant ON sessions(tenant, id);
CREATE TABLE IF NOT EXISTS credentials (session TEXT NOT NULL, binding TEXT NOT NULL, tenant TEXT NOT NULL, data BLOB NOT NULL, PRIMARY KEY(session,binding));`)
	if err != nil {
		db.Close()
		return nil, err
	}
	return &SQLite{db: db}, nil
}

func (s *SQLite) Close() error { return s.db.Close() }

func (s *SQLite) Create(ctx context.Context, session core.Session) error {
	data, err := json.Marshal(session)
	if err != nil {
		return err
	}
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err = tx.ExecContext(ctx, `INSERT INTO sessions(id,tenant,created,data) VALUES(?,?,?,?)`, session.ID, session.Tenant, session.CreatedAt.Format(time.RFC3339Nano), data); err != nil {
		return err
	}
	if err := insertCredentials(ctx, tx, session); err != nil {
		return err
	}
	return tx.Commit()
}

func (s *SQLite) Get(ctx context.Context, id string) (core.Session, error) {
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
	session.Credentials, err = s.credentials(ctx, session)
	return session, err
}

func (s *SQLite) Save(ctx context.Context, session core.Session) error {
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

func (s *SQLite) List(ctx context.Context, tenant, after string, limit int) ([]core.Session, error) {
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
func (s *SQLite) Scan(ctx context.Context, after string, limit int) ([]core.Session, error) {
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
func (s *SQLite) Import(ctx context.Context, session core.Session) error {
	data, err := json.Marshal(session)
	if err != nil {
		return err
	}
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	r, err := tx.ExecContext(ctx, `INSERT INTO sessions(id,tenant,created,data) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data WHERE sessions.tenant=excluded.tenant`, session.ID, session.Tenant, session.CreatedAt.Format(time.RFC3339Nano), data)
	if err != nil {
		return err
	}
	n, err := r.RowsAffected()
	if err == nil && n != 1 {
		return core.E(core.Forbidden, "session belongs to another tenant")
	}
	if err != nil {
		return err
	}
	if err := insertCredentials(ctx, tx, session); err != nil {
		return err
	}
	return tx.Commit()
}

// Existing private state wins during restore, including a revocation tombstone.
func insertCredentials(ctx context.Context, tx *sql.Tx, session core.Session) error {
	for _, record := range session.Credentials {
		data, err := json.Marshal(record)
		if err != nil {
			return err
		}
		if _, err := tx.ExecContext(ctx, `INSERT INTO credentials(session,binding,tenant,data) VALUES(?,?,?,?) ON CONFLICT(session,binding) DO NOTHING`, session.ID, record.Binding, session.Tenant, data); err != nil {
			return err
		}
	}
	return nil
}
func (s *SQLite) credentials(ctx context.Context, session core.Session) ([]core.CredentialRecord, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM credentials WHERE session=? AND tenant=? ORDER BY binding`, session.ID, session.Tenant)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := []core.CredentialRecord{}
	for rows.Next() {
		var data []byte
		if err := rows.Scan(&data); err != nil {
			return nil, err
		}
		var record core.CredentialRecord
		if err := json.Unmarshal(data, &record); err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, rows.Err()
}

// RevokeCredential retains a tombstone so old checkpoints cannot resurrect it.
func (s *SQLite) RevokeCredential(ctx context.Context, tenant, session, binding string) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	result, err := tx.ExecContext(ctx, `UPDATE credentials SET data=json_set(data,'$.revoked',json('true'),'$.ciphertext','') WHERE session=? AND binding=? AND tenant=?`, session, binding, tenant)
	if err != nil {
		return err
	}
	n, err := result.RowsAffected()
	if err == nil && n != 1 {
		return core.E(core.NotFound, "credential not found")
	}
	if err != nil {
		return err
	}
	if err := touchCredentials(ctx, tx, tenant, session); err != nil {
		return err
	}
	return tx.Commit()
}

func touchCredentials(ctx context.Context, tx *sql.Tx, tenant, id string) error {
	_, err := tx.ExecContext(ctx, `UPDATE sessions SET data=json_set(data,'$.updated_at',?) WHERE id=? AND tenant=?`, time.Now().UTC().Format(time.RFC3339Nano), id, tenant)
	return err
}

// RotateCredentials re-encrypts a session's records atomically; callers supply
// only ciphertext produced with the independent key provider.
func (s *SQLite) RotateCredentials(ctx context.Context, session core.Session) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	for _, record := range session.Credentials {
		if record.Revoked {
			continue
		}
		data, err := json.Marshal(record)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `UPDATE credentials SET data=? WHERE session=? AND binding=? AND tenant=? AND json_extract(data,'$.revoked') IS NOT 1`, data, session.ID, record.Binding, session.Tenant)
		if err != nil {
			return err
		}
	}
	if err := touchCredentials(ctx, tx, session.Tenant, session.ID); err != nil {
		return err
	}
	return tx.Commit()
}
