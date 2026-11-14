// Package postgres supports PostgreSQL, including Supabase's PostgreSQL endpoint.
package postgres

import (
	"context"
	"database/sql"
	"errors"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	_ "github.com/jackc/pgx/v5/stdlib"
	"strings"
	"time"
)

type Store struct{ db *sql.DB }

func New(ctx context.Context, dsn string) (*Store, error) {
	if dsn == "" {
		return nil, adapters.ErrInvalid
	}
	db, err := sql.Open("pgx", dsn)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(4)
	db.SetMaxIdleConns(1)
	db.SetConnMaxLifetime(10 * time.Minute)
	if _, err = db.ExecContext(ctx, `CREATE TABLE IF NOT EXISTS brain_objects(key TEXT PRIMARY KEY,version TEXT NOT NULL,data BYTEA NOT NULL)`); err != nil {
		db.Close()
		return nil, err
	}
	return &Store{db}, nil
}
func (s *Store) Get(ctx context.Context, key string) (adapters.Object, error) {
	if !adapters.ValidKey(key) {
		return adapters.Object{}, adapters.ErrInvalid
	}
	o := adapters.Object{Key: key}
	err := s.db.QueryRowContext(ctx, `SELECT version,data FROM brain_objects WHERE key=$1`, key).Scan(&o.Version, &o.Data)
	if errors.Is(err, sql.ErrNoRows) {
		err = adapters.ErrNotFound
	}
	return o, err
}
func (s *Store) Put(ctx context.Context, key string, data []byte, c adapters.Condition) (adapters.Object, error) {
	if err := adapters.Validate(key, data, c); err != nil {
		return adapters.Object{}, err
	}
	if data == nil {
		data = []byte{}
	}
	v := adapters.Version()
	var r sql.Result
	var err error
	if c.CreateOnly {
		r, err = s.db.ExecContext(ctx, `INSERT INTO brain_objects(key,version,data) VALUES($1,$2,$3) ON CONFLICT(key) DO NOTHING`, key, v, data)
	} else if c.Version != "" {
		r, err = s.db.ExecContext(ctx, `UPDATE brain_objects SET version=$1,data=$2 WHERE key=$3 AND version=$4`, v, data, key, c.Version)
	} else {
		r, err = s.db.ExecContext(ctx, `INSERT INTO brain_objects(key,version,data) VALUES($1,$2,$3) ON CONFLICT(key) DO UPDATE SET version=EXCLUDED.version,data=EXCLUDED.data`, key, v, data)
	}
	if err != nil {
		return adapters.Object{}, err
	}
	n, err := r.RowsAffected()
	if err != nil {
		return adapters.Object{}, err
	}
	if n != 1 {
		return adapters.Object{}, adapters.ErrConflict
	}
	return adapters.Object{Key: key, Data: data, Version: v}, nil
}
func (s *Store) Delete(ctx context.Context, key string) error {
	if !adapters.ValidKey(key) {
		return adapters.ErrInvalid
	}
	_, err := s.db.ExecContext(ctx, `DELETE FROM brain_objects WHERE key=$1`, key)
	return err
}
func (s *Store) List(ctx context.Context, prefix, after string, limit int) (adapters.Page, error) {
	if err := adapters.ValidateList(prefix, after, limit); err != nil {
		return adapters.Page{}, err
	}
	pattern := strings.NewReplacer("\\", "\\\\", "%", "\\%", "_", "\\_").Replace(prefix) + "%"
	rows, err := s.db.QueryContext(ctx, `SELECT key,version FROM brain_objects WHERE key>$1 AND key LIKE $2 ESCAPE '\' ORDER BY key LIMIT $3`, after, pattern, limit+1)
	if err != nil {
		return adapters.Page{}, err
	}
	defer rows.Close()
	p := adapters.Page{Objects: []adapters.Object{}}
	for rows.Next() {
		var o adapters.Object
		if err = rows.Scan(&o.Key, &o.Version); err != nil {
			return p, err
		}
		if len(p.Objects) == limit {
			p.Next = p.Objects[limit-1].Key
			break
		}
		p.Objects = append(p.Objects, o)
	}
	return p, rows.Err()
}
func (s *Store) Close() error { return s.db.Close() }
