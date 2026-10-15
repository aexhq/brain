// Package sqlcontrol implements the control port with SQLite FULL transactions
// or PostgreSQL. No external effect is issued inside a database transaction.
package sqlcontrol

import (
	"context"
	"database/sql"
	_ "embed"
	"encoding/json"
	"errors"
	"math"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"sync"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/jackc/pgx/v5/pgconn"
	_ "github.com/jackc/pgx/v5/stdlib"
	_ "modernc.org/sqlite"
)

//go:embed schema.sql
var schema string

type Store struct {
	db       *sql.DB
	postgres bool
	fences   sync.Map
}

type queryer interface {
	ExecContext(context.Context, string, ...any) (sql.Result, error)
	QueryContext(context.Context, string, ...any) (*sql.Rows, error)
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

func Open(ctx context.Context, dsn string, development bool) (*Store, error) {
	driver := "sqlite"
	pg := strings.HasPrefix(dsn, "postgres://") || strings.HasPrefix(dsn, "postgresql://")
	if pg {
		driver = "pgx"
		u, err := url.Parse(dsn)
		if err != nil {
			return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid PostgreSQL URL")
		}
		if !development && u.Query().Get("sslmode") != "verify-full" {
			return nil, kernel.Fail(wire.ErrorCodeInvalid, "PostgreSQL requires sslmode=verify-full outside explicit development mode")
		}
	} else {
		separator := "?"
		if strings.Contains(dsn, "?") {
			separator = "&"
		}
		dsn += separator + "_pragma=journal_mode(WAL)&_pragma=synchronous(FULL)&_pragma=foreign_keys(ON)&_pragma=busy_timeout(5000)&_txlock=immediate"
	}
	db, err := sql.Open(driver, dsn)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	if pg {
		db.SetMaxOpenConns(16)
		db.SetMaxIdleConns(4)
	}
	s := &Store{db: db, postgres: pg}
	if err := db.PingContext(ctx); err != nil {
		db.Close()
		return nil, kernel.Fail(wire.ErrorCodeUnavailable, "control database is unavailable")
	}
	err = s.transact(ctx, true, func(tx *sql.Tx) error {
		for _, statement := range strings.Split(schema, ";") {
			if strings.TrimSpace(statement) != "" {
				if _, err := tx.ExecContext(ctx, statement); err != nil {
					return err
				}
			}
		}
		var version int
		if err := tx.QueryRowContext(ctx, "SELECT MAX(version) FROM brain_schema").Scan(&version); err != nil {
			return err
		}
		if version == 1 {
			for _, statement := range []string{
				"ALTER TABLE commands ADD COLUMN kind TEXT NOT NULL DEFAULT ''",
				"ALTER TABLE allocations ADD COLUMN archive_key TEXT NOT NULL DEFAULT ''",
				"ALTER TABLE operations ADD COLUMN intent_sequence BIGINT NOT NULL DEFAULT 0",
				"ALTER TABLE operations ADD COLUMN intent_digest TEXT NOT NULL DEFAULT ''",
			} {
				if _, err := tx.ExecContext(ctx, statement); err != nil {
					return err
				}
			}
			expression := "json_extract(payload,'$.kind')"
			if s.postgres {
				expression = "CAST(payload AS JSONB)->>'kind'"
			}
			if _, err := tx.ExecContext(ctx, "UPDATE commands SET kind="+expression); err != nil {
				return err
			}
			if _, err := tx.ExecContext(ctx, "INSERT INTO brain_schema VALUES(2)"); err != nil {
				return err
			}
			version = 2
		}
		if version == 2 {
			for _, statement := range []string{
				"ALTER TABLE sessions ADD COLUMN ready BIGINT NOT NULL DEFAULT 0",
				"ALTER TABLE sessions ADD COLUMN ready_ms BIGINT NOT NULL DEFAULT 0",
				"CREATE INDEX sessions_ready ON sessions(ready,tenant,ready_ms,id)",
				"UPDATE sessions SET ready=1 WHERE lifecycle<>'deleted'",
				"INSERT INTO brain_schema VALUES(3)",
			} {
				if _, err := tx.ExecContext(ctx, statement); err != nil {
					return err
				}
			}
			version = 3
		}
		if version == 3 {
			for _, statement := range []string{
				"ALTER TABLE nodes ADD COLUMN endpoint TEXT NOT NULL DEFAULT ''",
				"CREATE TABLE IF NOT EXISTS cluster_configuration(key TEXT PRIMARY KEY,value TEXT NOT NULL)",
				"INSERT INTO brain_schema VALUES(4)",
			} {
				if _, err := tx.ExecContext(ctx, statement); err != nil {
					return err
				}
			}
			version = 4
		}
		if version == 4 {
			for _, statement := range []string{
				"ALTER TABLE nodes ADD COLUMN configuration_digest TEXT NOT NULL DEFAULT ''",
				"CREATE TABLE IF NOT EXISTS node_capabilities(node_id TEXT NOT NULL,capability TEXT NOT NULL,PRIMARY KEY(node_id,capability))",
				"CREATE TABLE IF NOT EXISTS session_requirements(session_id TEXT NOT NULL,capability TEXT NOT NULL,PRIMARY KEY(session_id,capability))",
				"INSERT INTO session_requirements SELECT id,'legacy:verified-offline-resource-adoption-required' FROM sessions",
				"INSERT INTO brain_schema VALUES(5)",
			} {
				if _, err := tx.ExecContext(ctx, statement); err != nil {
					return err
				}
			}
			version = 5
		}
		if version != 11 {
			return kernel.Fail(wire.ErrorCodeUnsupported, "control database requires verified offline migration")
		}
		if _, err := tx.ExecContext(ctx, "CREATE INDEX IF NOT EXISTS sessions_ready ON sessions(ready,tenant,ready_ms,id)"); err != nil {
			return err
		}
		return nil
	})
	if err != nil {
		db.Close()
		return nil, err
	}
	return s, nil
}

func (s *Store) Close() error { return s.db.Close() }

func (s *Store) q(query string) string {
	if !s.postgres {
		return query
	}
	n := 0
	return replaceParameters(query, &n)
}

func replaceParameters(query string, n *int) string {
	parts := strings.Split(query, "?")
	for i := 1; i < len(parts); i++ {
		*n++
		parts[i] = "$" + strconv.Itoa(*n) + parts[i]
	}
	return strings.Join(parts, "")
}

func (s *Store) clock() string {
	if s.postgres {
		return "CAST(FLOOR(EXTRACT(EPOCH FROM clock_timestamp()) * 1000) AS BIGINT)"
	}
	return "CAST(unixepoch('subsec') * 1000 AS INTEGER)"
}

func (s *Store) lock() string {
	if s.postgres {
		return " FOR UPDATE"
	}
	return ""
}

func (s *Store) Clock(ctx context.Context) (int64, error) { return s.now(ctx, s.db) }

func (s *Store) now(ctx context.Context, q queryer) (int64, error) {
	var now int64
	err := q.QueryRowContext(ctx, "SELECT "+s.clock()).Scan(&now)
	return now, err
}

func (s *Store) transaction(ctx context.Context, f func(*sql.Tx) error) error {
	return s.transact(ctx, false, f)
}

func (s *Store) transact(ctx context.Context, migration bool, f func(*sql.Tx) error) error {
	isolation := sql.LevelSerializable
	if s.postgres {
		// Session-row locks serialize domain mutations; shared budgets use
		// conditional updates. Fresh statement snapshots avoid stale lease reads.
		isolation = sql.LevelReadCommitted
	}
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: isolation})
	if err != nil {
		return conflict(err)
	}
	defer tx.Rollback()
	fences := map[string]bool{}
	s.fences.Store(tx, fences)
	defer s.fences.Delete(tx)
	if migration && s.postgres {
		if _, err := tx.ExecContext(ctx, "SELECT pg_advisory_xact_lock(726491320105)"); err != nil {
			return conflict(err)
		}
	}
	if err := s.authorize(ctx, tx, false); err != nil {
		return conflict(err)
	}
	if err := f(tx); err != nil {
		return conflict(err)
	}
	if err := s.authorize(ctx, tx, true); err != nil {
		return conflict(err)
	}
	for node := range fences {
		var live int
		if err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM nodes WHERE id=? AND lease>"+s.clock()+" AND draining=0"), node).Scan(&live); err != nil {
			return conflict(err)
		}
		if live != 1 {
			return kernel.Fail(wire.ErrorCodeConflict, "node lease expired before the authority transaction committed")
		}
	}
	if err := tx.Commit(); err != nil {
		if knownRollback(err) {
			return conflict(err)
		}
		return kernel.Fail(wire.ErrorCodeUncertain, "control commit outcome is uncertain; reconcile the original request identity")
	}
	return nil
}

func knownRollback(err error) bool {
	var pg *pgconn.PgError
	return errors.As(err, &pg) && (pg.Code == "40001" || pg.Code == "40P01" || pg.Code == "55P03" || pg.Code == "23505")
}

func conflict(err error) error {
	if knownRollback(err) {
		return kernel.Fail(wire.ErrorCodeConflict, "database rejected a conflicting control transaction; no dispatch permission was granted")
	}
	return err
}

func requireOne(result sql.Result, err error) error {
	if err != nil {
		return err
	}
	n, err := result.RowsAffected()
	if err == nil && n != 1 {
		return kernel.Fail(wire.ErrorCodeConflict, "state, revision, lease or authority changed")
	}
	return err
}

func notFound(err error) error {
	if errors.Is(err, sql.ErrNoRows) {
		return kernel.Fail(wire.ErrorCodeNotFound, "resource not found")
	}
	return err
}

func decode[T any](text string) (T, error) {
	var value T
	err := json.Unmarshal([]byte(text), &value)
	if err != nil {
		return value, kernel.Fail(wire.ErrorCodeCorrupt, "control data does not match its contract")
	}
	return value, nil
}

const sessionColumns = "id,tenant,config,lifecycle,revision,sequence,owner,epoch,generation,paused,created_ms,parent,budget"

type scanner interface{ Scan(...any) error }

func scanSession(row scanner) (control.Session, error) {
	var out control.Session
	var config, lifecycle, parent string
	var revision, sequence, created int64
	var paused int
	err := row.Scan(&out.View.ID, &out.Tenant, &config, &lifecycle, &revision, &sequence,
		&out.Owner.Node, &out.Owner.Epoch, &out.Generation, &paused, &created, &parent, &out.Budget)
	if err != nil {
		return out, notFound(err)
	}
	out.View.Config, err = decode[wire.SessionConfig](config)
	out.View.Lifecycle = wire.Lifecycle(lifecycle)
	out.View.Revision = strconv.FormatInt(revision, 10)
	out.View.Sequence = strconv.FormatInt(sequence, 10)
	out.View.OwnerEpoch = strconv.FormatInt(out.Owner.Epoch, 10)
	out.View.CreatedMs = uint64(created)
	out.View.Paused = paused != 0
	if parent != "" {
		out.View.Parent = &parent
	}
	return out, err
}

func (s *Store) session(ctx context.Context, q queryer, id string, lock bool) (control.Session, error) {
	query := "SELECT " + sessionColumns + " FROM sessions WHERE id=?"
	if lock {
		query += s.lock()
	}
	return scanSession(q.QueryRowContext(ctx, s.q(query), id))
}

func (s *Store) guard(ctx context.Context, tx *sql.Tx, session string, owner control.Owner, dispatch bool) (control.Session, error) {
	value, err := s.session(ctx, tx, session, true)
	if err != nil {
		return value, err
	}
	var live int
	err = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM nodes WHERE id=? AND lease>"+s.clock()+" AND draining=0"), owner.Node).Scan(&live)
	if err != nil {
		return value, err
	}
	if live != 1 || value.Owner != owner || value.View.Lifecycle == wire.LifecycleDeleted {
		return value, kernel.Fail(wire.ErrorCodeConflict, "session owner is fenced")
	}
	s.fence(tx, owner.Node)
	if dispatch && (value.View.Lifecycle != wire.LifecycleOpen || value.View.Paused) {
		return value, kernel.Fail(wire.ErrorCodeConflict, "session is not admitting effects")
	}
	if dispatch {
		if err = s.unfrozen(ctx, tx, session, false); err != nil {
			return value, err
		}
		if err = s.liveLineage(ctx, tx, value, true); err != nil {
			return value, err
		}
	}
	return value, nil
}

func (s *Store) fence(q queryer, node string) {
	if tx, ok := q.(*sql.Tx); ok {
		if values, ok := s.fences.Load(tx); ok {
			values.(map[string]bool)[node] = true
		}
	}
}

func (s *Store) RegisterNode(ctx context.Context, node string, lease int64) error {
	_, err := s.db.ExecContext(ctx, s.q("INSERT INTO nodes(id,lease) VALUES(?,"+s.clock()+"+?)"), node, lease)
	return err
}

func normalizedCapabilities(values []string) ([]string, error) {
	if len(values) > 4096 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "node or session capability catalogue exceeds its bound")
	}
	out := slices.Clone(values)
	for _, value := range out {
		if !kernel.Identifier(value) {
			return nil, kernel.Fail(wire.ErrorCodeInvalid, "capability identifiers must be bounded identifiers")
		}
	}
	slices.Sort(out)
	return slices.Compact(out), nil
}

func (s *Store) ConfigureNode(ctx context.Context, node, endpoint, custodyDigest string, capabilities []string) error {
	capabilities, err := normalizedCapabilities(capabilities)
	if err != nil {
		return err
	}
	configuration := kernel.Digest(kernel.JSON(struct {
		Endpoint     string
		Capabilities []string
	}{endpoint, capabilities}))
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := tx.ExecContext(ctx, s.q("INSERT INTO cluster_configuration(key,value) VALUES('custody',?) ON CONFLICT DO NOTHING"), custodyDigest); err != nil {
			return err
		}
		var expected string
		if err := tx.QueryRowContext(ctx, "SELECT value FROM cluster_configuration WHERE key='custody'").Scan(&expected); err != nil {
			return err
		}
		if expected != custodyDigest {
			return kernel.Fail(wire.ErrorCodeForbidden, "control store requires its original retained cluster custody key")
		}
		if err := requireOne(tx.ExecContext(ctx, s.q("UPDATE nodes SET endpoint=?,configuration_digest=? WHERE id=? AND (configuration_digest='' OR configuration_digest=?) AND lease>"+s.clock()+" AND draining=0"),
			endpoint, configuration, node, configuration)); err != nil {
			return err
		}
		for _, capability := range capabilities {
			if _, err := tx.ExecContext(ctx, s.q("INSERT INTO node_capabilities VALUES(?,?) ON CONFLICT DO NOTHING"), node, capability); err != nil {
				return err
			}
		}
		return nil
	})
}

func (s *Store) RenewNode(ctx context.Context, node string, lease int64) error {
	return requireOne(s.db.ExecContext(ctx, s.q("UPDATE nodes SET lease="+s.clock()+"+? WHERE id=? AND lease>"+s.clock()+" AND draining=0"), lease, node))
}

func (s *Store) RetireNode(ctx context.Context, node string) error {
	_, err := s.db.ExecContext(ctx, s.q("UPDATE nodes SET draining=1,lease=0 WHERE id=?"), node)
	return err
}

func (s *Store) Create(ctx context.Context, tenant string, request wire.SessionCreate, parent, budget string, requirements []string) (out control.Session, err error) {
	return s.create(ctx, tenant, request, parent, budget, requirements, nil)
}

func (s *Store) Delegate(ctx context.Context, parent string, owner control.Owner, run string, step, index uint64, request wire.SessionCreate, requirements []string) (control.Session, error) {
	current, err := s.Session(ctx, "", parent)
	if err != nil {
		return control.Session{}, err
	}
	request.RequestID = kernel.ChildCreateID(parent, run, step, index)
	return s.create(ctx, current.Tenant, request, parent, current.Budget, requirements, &control.ChildOrigin{Owner: owner, Run: run, Step: step, Index: index})
}

func (s *Store) create(ctx context.Context, tenant string, request wire.SessionCreate, parent, budget string, requirements []string, origin *control.ChildOrigin) (out control.Session, err error) {
	requirements, err = normalizedCapabilities(requirements)
	if err != nil {
		return out, err
	}
	id := kernel.ID("session", tenant, request.RequestID)
	hash := kernel.Digest(kernel.JSON(request))
	if parent != "" {
		hash = kernel.Digest(kernel.JSON([]any{request, parent, budget}))
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		var ancestor control.Session
		depth := 0
		if parent != "" {
			var e error
			ancestor, e = s.session(ctx, tx, parent, true)
			if e != nil {
				return e
			}
			if ancestor.Tenant != tenant || ancestor.Budget != budget || ancestor.View.Lifecycle != wire.LifecycleOpen {
				return kernel.Fail(wire.ErrorCodeForbidden, "delegation parent or shared authority is not live")
			}
			if e = s.liveLineage(ctx, tx, ancestor, false); e != nil {
				return e
			}
			if origin != nil {
				if _, e = s.guard(ctx, tx, parent, origin.Owner, true); e != nil {
					return e
				}
				var body string
				if e = tx.QueryRowContext(ctx, s.q(`SELECT decision FROM runs WHERE session_id=? AND id=? AND step=?
AND status='active' AND decision<>''`), parent, origin.Run, origin.Step).Scan(&body); errors.Is(e, sql.ErrNoRows) {
					return kernel.Fail(wire.ErrorCodeConflict, "delegating run no longer holds this decision boundary")
				}
				if e != nil {
					return e
				}
				decision, e := decode[wire.Decision](body)
				if e != nil {
					return e
				}
				if e = kernel.ValidateChildDecision(decision, origin.Index, request); e != nil {
					return e
				}
			}
			if e = kernel.ValidateDelegation(ancestor.View.Config, request.Config); e != nil {
				return e
			}
			if e = tx.QueryRowContext(ctx, s.q("SELECT MAX(depth)+1 FROM budget_members WHERE session_id=?"), parent).Scan(&depth); e != nil {
				return e
			}
			if depth > kernel.MaxDelegationDepth {
				return kernel.Fail(wire.ErrorCodeForbidden, "delegation depth limit reached")
			}
		} else if budget != "" {
			return kernel.Fail(wire.ErrorCodeInvalid, "a shared budget requires its admitted parent")
		}
		var existing string
		e := tx.QueryRowContext(ctx, s.q("SELECT create_hash FROM sessions WHERE tenant=? AND create_key=?"), tenant, request.RequestID).Scan(&existing)
		if e == nil {
			if existing != hash {
				return kernel.Fail(wire.ErrorCodeConflict, "idempotency key has different session arguments")
			}
			if origin != nil {
				var linked string
				e = tx.QueryRowContext(ctx, s.q("SELECT child_id FROM run_children WHERE session_id=? AND run_id=? AND step=? AND ordinal=?"),
					parent, origin.Run, origin.Step, origin.Index).Scan(&linked)
				if e != nil || linked != id {
					return kernel.Fail(wire.ErrorCodeConflict, "child identity does not match its original decision slot")
				}
			}
			out, e = s.session(ctx, tx, id, false)
			return e
		}
		if !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		if parent == "" {
			budget = id
		}
		limits := request.Config.Limits
		tokens := int64(-1)
		if limits.Tokens != nil {
			tokens = int64(*limits.Tokens)
		}
		_, e = tx.ExecContext(ctx, s.q("INSERT INTO budgets(id,max_calls,max_active,max_bytes,max_tokens) VALUES(?,?,?,?,?)"),
			id, limits.Calls, limits.Concurrent, limits.RetainedBytes, tokens)
		if e != nil {
			return e
		}
		if parent == "" {
			if _, e = tx.ExecContext(ctx, s.q("INSERT INTO tree_authority VALUES(?)"), id); e != nil {
				return e
			}
		}
		if _, e = tx.ExecContext(ctx, s.q("INSERT INTO budget_members VALUES(?,?,?,1)"), id, id, depth); e != nil {
			return e
		}
		if parent != "" {
			if _, e = tx.ExecContext(ctx, s.q(`INSERT INTO budget_members SELECT ?,m.budget_id,m.depth,b.cancel_generation
FROM budget_members m JOIN budgets b ON b.id=m.budget_id WHERE m.session_id=?`), id, parent); e != nil {
				return e
			}
		}
		_, e = tx.ExecContext(ctx, s.q("INSERT INTO sessions(id,tenant,create_key,create_hash,config,created_ms,parent,budget) VALUES(?,?,?,?,?,"+s.clock()+",?,?)"),
			id, tenant, request.RequestID, hash, string(kernel.JSON(request.Config)), parent, budget)
		if e != nil {
			return e
		}
		if parent != "" {
			link := map[string]any{"child_id": id, "shared_budget": budget}
			if origin != nil {
				link["run_id"], link["step"], link["index"] = origin.Run, origin.Step, origin.Index
			}
			if _, e = s.accept(ctx, tx, kernel.Principal{Tenant: tenant, Subject: "kernel"}, parent, wire.Input{
				RequestID: "delegated:" + id, Data: wire.InputData{Kind: "journal", Name: kernel.Ptr("session.delegated"),
					Data: kernel.JSON(link)},
			}); e != nil {
				return e
			}
		}
		for _, capability := range requirements {
			if _, e = tx.ExecContext(ctx, s.q("INSERT INTO session_requirements VALUES(?,?)"), id, capability); e != nil {
				return e
			}
		}
		if request.Message != nil {
			input := wire.Input{RequestID: request.RequestID + ":initial", Data: wire.InputData{Kind: "message", Text: request.Message}}
			if _, e = s.accept(ctx, tx, kernel.Principal{Tenant: tenant, Subject: "creator"}, id, input); e != nil {
				return e
			}
		}
		if origin != nil {
			_, e = tx.ExecContext(ctx, s.q("INSERT INTO run_children VALUES(?,?,?,?,?,?)"),
				parent, origin.Run, origin.Step, origin.Index, id, kernel.ID("input", id, request.RequestID+":initial"))
			if e != nil {
				return e
			}
		}
		out, e = s.session(ctx, tx, id, false)
		return e
	})
	return
}

func (s *Store) Session(ctx context.Context, tenant, id string) (control.Session, error) {
	value, err := s.session(ctx, s.db, id, false)
	if err == nil && ((tenant != "" && value.Tenant != tenant) || value.View.Lifecycle == wire.LifecycleDeleted) {
		err = kernel.Fail(wire.ErrorCodeNotFound, "session not found")
	}
	return value, err
}

func (s *Store) List(ctx context.Context, tenant, after string, limit int) ([]control.Session, error) {
	rows, err := s.pageRows(ctx, sessionColumns, "sessions", "tenant=? AND id>? AND lifecycle<>'deleted'", "id", s.bytes("config"), limit, tenant, after)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Session{}
	for rows.Next() {
		item, err := scanSession(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, item)
	}
	return out, rows.Err()
}

func (s *Store) ClaimOwner(ctx context.Context, session, node string) (out control.Owner, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.session(ctx, tx, session, true)
		if e != nil {
			return e
		}
		now, e := s.now(ctx, tx)
		if e != nil {
			return e
		}
		var live int
		e = tx.QueryRowContext(ctx, s.q(`SELECT COUNT(*) FROM nodes n WHERE n.id=? AND n.lease>? AND n.draining=0
AND NOT EXISTS(SELECT 1 FROM session_requirements r WHERE r.session_id=?
AND NOT EXISTS(SELECT 1 FROM node_capabilities c WHERE c.node_id=n.id AND c.capability=r.capability))`), node, now, session).Scan(&live)
		if e != nil || live != 1 {
			return kernel.Fail(wire.ErrorCodeConflict, "node has no live eligible lease for the admitted capabilities and retained resources")
		}
		s.fence(tx, node)
		if current.Owner.Node == node {
			out = current.Owner
			return nil
		}
		e = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM nodes WHERE id=? AND lease>? AND draining=0"), current.Owner.Node, now).Scan(&live)
		if e != nil {
			return e
		}
		if live != 0 {
			return kernel.Fail(wire.ErrorCodeConflict, "session has a live owner")
		}
		if current.Owner.Epoch == math.MaxInt64 {
			return kernel.Fail(wire.ErrorCodeOverloaded, "owner epoch exhausted")
		}
		out = control.Owner{Node: node, Epoch: current.Owner.Epoch + 1}
		if _, e = tx.ExecContext(ctx, s.q("UPDATE ownership_transfers SET phase='failed',error=?,updated_ms="+s.clock()+" WHERE session_id=? AND "+transferActive),
			string(kernel.JSON(kernel.Response(kernel.Fail(wire.ErrorCodeUncertain, "source incarnation was lost before planned transfer completed")))), session); e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, s.q("UPDATE allocations SET status='unresolved' WHERE session_id=? AND status='reserved' AND epoch<?"), session, out.Epoch); e != nil {
			return e
		}
		return requireOne(tx.ExecContext(ctx, s.q("UPDATE sessions SET owner=?,epoch=? WHERE id=? AND epoch=? AND lifecycle<>'deleted'"), node, out.Epoch, session, current.Owner.Epoch))
	})
	return
}

func (s *Store) Allocate(ctx context.Context, session string, owner control.Owner) (sequence int64, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}
		if err := s.unfrozen(ctx, tx, session, true); err != nil {
			return err
		}
		if err := tx.QueryRowContext(ctx, s.q("UPDATE sessions SET sequence=sequence+1 WHERE id=? AND sequence<9223372036854775807 RETURNING sequence"), session).Scan(&sequence); err != nil {
			return err
		}
		_, err := tx.ExecContext(ctx, s.q("INSERT INTO allocations(session_id,sequence,owner,epoch) VALUES(?,?,?,?)"), session, sequence, owner.Node, owner.Epoch)
		return err
	})
	return
}

func (s *Store) Written(ctx context.Context, session string, owner control.Owner, sequence int64, digest string, bytes int64) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}

		return requireOne(tx.ExecContext(ctx, s.q("UPDATE allocations SET status='written',digest=?,bytes=? WHERE session_id=? AND sequence=? AND owner=? AND epoch=? AND (status='reserved' OR (status='written' AND digest=?))"),
			digest, bytes, session, sequence, owner.Node, owner.Epoch, digest))
	})
}

func (s *Store) SealUnused(ctx context.Context, session string, owner control.Owner, sequence int64) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}
		return requireOne(tx.ExecContext(ctx, s.q("UPDATE allocations SET status='unused' WHERE session_id=? AND sequence=? AND owner=? AND epoch=? AND status='reserved'"), session, sequence, owner.Node, owner.Epoch))
	})
}

func (s *Store) Allocations(ctx context.Context, session string, after int64, limit int) ([]control.Allocation, int64, error) {
	var high int64
	if err := s.db.QueryRowContext(ctx, s.q("SELECT sequence FROM sessions WHERE id=?"), session).Scan(&high); err != nil {
		return nil, 0, notFound(err)
	}
	rows, err := s.db.QueryContext(ctx, s.q("SELECT sequence,status,digest,bytes FROM allocations WHERE session_id=? AND sequence>? AND sequence<=? ORDER BY sequence LIMIT ?"), session, after, high, limit)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()
	out := []control.Allocation{}
	for rows.Next() {
		var a control.Allocation
		if err := rows.Scan(&a.Sequence, &a.Status, &a.Digest, &a.Bytes); err != nil {
			return nil, 0, err
		}
		out = append(out, a)
	}
	return out, high, rows.Err()
}

func (s *Store) LocateRecord(ctx context.Context, session string, sequence int64) (control.Allocation, error) {
	var a control.Allocation
	err := s.db.QueryRowContext(ctx, s.q(`SELECT a.sequence,a.status,a.digest,a.bytes,a.owner,COALESCE(n.endpoint,''),s.owner
FROM allocations a JOIN sessions s ON s.id=a.session_id LEFT JOIN nodes n ON n.id=a.owner WHERE a.session_id=? AND a.sequence=?`),
		session, sequence).Scan(&a.Sequence, &a.Status, &a.Digest, &a.Bytes, &a.Node, &a.Endpoint, &a.CurrentOwner)
	return a, notFound(err)
}

func (s *Store) Pending(ctx context.Context, node string, limit int) ([]string, error) {
	query := `SELECT id FROM (
 SELECT s.id,s.ready_ms,ROW_NUMBER() OVER(PARTITION BY s.tenant ORDER BY s.ready_ms,s.id) AS tenant_turn
 FROM sessions s WHERE s.ready=1 AND s.lifecycle<>'deleted'
 AND (NOT EXISTS(SELECT 1 FROM ownership_transfers t WHERE t.session_id=s.id AND t.phase IN ('draining','verifying'))
  OR NOT EXISTS(SELECT 1 FROM nodes n WHERE n.id=s.owner AND n.lease>` + s.clock() + ` AND n.draining=0))
 AND NOT EXISTS(SELECT 1 FROM session_requirements r WHERE r.session_id=s.id
   AND NOT EXISTS(SELECT 1 FROM node_capabilities c WHERE c.node_id=? AND c.capability=r.capability))
 AND (s.owner=? OR NOT EXISTS(SELECT 1 FROM nodes n WHERE n.id=s.owner AND n.lease>` + s.clock() + ` AND n.draining=0))
) ready_sessions ORDER BY tenant_turn,ready_ms,id LIMIT ?`
	rows, err := s.db.QueryContext(ctx, s.q(query), node, node, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []string{}
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return nil, err
		}
		out = append(out, id)
	}
	return out, rows.Err()
}

func (s *Store) FinishStep(ctx context.Context, session string, owner control.Owner) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}
		ready := `EXISTS(SELECT 1 FROM ownership_transfers t WHERE t.session_id=sessions.id AND t.phase IN ('draining','verifying'))
OR EXISTS(SELECT 1 FROM commands c WHERE c.session_id=sessions.id AND c.status='accepted')
OR EXISTS(SELECT 1 FROM operations o WHERE o.session_id=sessions.id AND o.result_sequence=0 AND o.status IN ('completed','failed','unknown'))
OR (sessions.paused=0 AND EXISTS(
 SELECT 1 FROM runs r WHERE r.session_id=sessions.id AND (
  r.status IN ('queued','active')
  OR (r.status='waiting' AND (
   (r.wait_kind='operation' AND EXISTS(SELECT 1 FROM operations o WHERE o.id=r.wait_id AND o.status IN ('completed','failed','unknown')))
   OR (r.wait_kind='interaction' AND EXISTS(SELECT 1 FROM interactions i WHERE i.id=r.wait_id AND i.status<>'pending'))
   OR (r.wait_kind='child' AND NOT EXISTS(SELECT 1 FROM runs child WHERE child.session_id=r.wait_id AND child.status IN ('queued','active','waiting','paused'))
   AND NOT EXISTS(SELECT 1 FROM commands child WHERE child.session_id=r.wait_id AND child.status='accepted' AND child.kind IN ('message','action','event')))
   OR (` + childrenReady + `)
  ))
 )
))`
		_, err := tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=CASE WHEN "+ready+" THEN 1 ELSE 0 END,ready_ms="+s.clock()+" WHERE id=?"), session)
		return err
	})
}

func (s *Store) Report(ctx context.Context) (wire.StorageReport, error) {
	out := wire.StorageReport{Format: "brain-data/4", Accepting: true}
	err := s.db.QueryRowContext(ctx, "SELECT COALESCE(SUM(b.retained),0),COALESCE(SUM(b.reserved),0) FROM budgets b JOIN sessions s ON s.id=b.id WHERE s.parent=''").Scan(&out.RetainedBytes, &out.ReservedOutputBytes)
	if err != nil {
		return out, err
	}
	err = s.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM sessions WHERE lifecycle<>'deleted'").Scan(&out.Sessions)
	return out, err
}
