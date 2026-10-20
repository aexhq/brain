package persistence

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync"
	"time"

	"github.com/aexhq/brain/internal/core"
	_ "github.com/jackc/pgx/v5/stdlib"
)

// Metadata is an eventual discovery index, never a session ownership lock.
type Metadata interface {
	Delete(context.Context, string, string) error
	Put(context.Context, core.Session, string) error
	Get(context.Context, string, string) (core.Session, string, error)
	List(context.Context, string, string, int) ([]core.Session, error)
	Close() error
}

type Postgres struct {
	db    *sql.DB
	mu    sync.Mutex
	ready bool
}

func NewPostgres(dsn string) (*Postgres, error) {
	if dsn == "" {
		return nil, errors.New("PostgreSQL connection string is missing")
	}
	db, err := sql.Open("pgx", dsn)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(4)
	db.SetMaxIdleConns(1)
	db.SetConnMaxLifetime(10 * time.Minute)
	return &Postgres{db: db}, nil
}
func (p *Postgres) ensure(ctx context.Context) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.ready {
		return nil
	}
	_, err := p.db.ExecContext(ctx, `CREATE TABLE IF NOT EXISTS brain_sessions(id TEXT PRIMARY KEY,tenant TEXT NOT NULL,data JSONB NOT NULL,checkpoint TEXT NOT NULL); CREATE INDEX IF NOT EXISTS brain_sessions_tenant ON brain_sessions(tenant,id)`)
	if err == nil {
		p.ready = true
	}
	return err
}
func (p *Postgres) Put(ctx context.Context, s core.Session, reference string) error {
	if err := p.ensure(ctx); err != nil {
		return err
	}
	b, err := json.Marshal(s)
	if err != nil {
		return err
	}
	r, err := p.db.ExecContext(ctx, `INSERT INTO brain_sessions(id,tenant,data,checkpoint) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO UPDATE SET data=EXCLUDED.data,checkpoint=EXCLUDED.checkpoint WHERE brain_sessions.tenant=EXCLUDED.tenant`, s.ID, s.Tenant, b, reference)
	if err != nil {
		return err
	}
	n, err := r.RowsAffected()
	if err == nil && n != 1 {
		return core.E(core.Forbidden, "metadata tenant mismatch")
	}
	return err
}
func (p *Postgres) Get(ctx context.Context, tenant, id string) (core.Session, string, error) {
	var s core.Session
	var reference string
	var data []byte
	if err := p.ensure(ctx); err != nil {
		return s, "", err
	}
	err := p.db.QueryRowContext(ctx, `SELECT data,checkpoint FROM brain_sessions WHERE tenant=$1 AND id=$2`, tenant, id).Scan(&data, &reference)
	if errors.Is(err, sql.ErrNoRows) {
		return s, "", core.E(core.NotFound, "session not found")
	}
	if err != nil {
		return s, "", err
	}
	if err := json.Unmarshal(data, &s); err != nil {
		return s, "", err
	}
	s.Tenant = tenant
	return s, reference, nil
}
func (p *Postgres) List(ctx context.Context, tenant, after string, limit int) ([]core.Session, error) {
	if limit < 1 || limit > 1000 {
		return nil, core.E(core.Invalid, "invalid list limit")
	}
	if err := p.ensure(ctx); err != nil {
		return nil, err
	}
	rows, err := p.db.QueryContext(ctx, `SELECT data FROM brain_sessions WHERE tenant=$1 AND id>$2 ORDER BY id LIMIT $3`, tenant, after, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := []core.Session{}
	for rows.Next() {
		var data []byte
		if err := rows.Scan(&data); err != nil {
			return nil, err
		}
		var s core.Session
		if err := json.Unmarshal(data, &s); err != nil {
			return nil, err
		}
		s.Tenant = tenant
		result = append(result, s)
	}
	return result, rows.Err()
}
func (p *Postgres) Close() error { return p.db.Close() }

func (p *Postgres) Delete(ctx context.Context, tenant, id string) error {
	if err := p.ensure(ctx); err != nil {
		return err
	}
	_, err := p.db.ExecContext(ctx, `DELETE FROM brain_sessions WHERE tenant=$1 AND id=$2`, tenant, id)
	return err
}
