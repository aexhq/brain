package sqlcontrol

import (
	"context"
	"database/sql"
	"errors"
	"strconv"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) liveSession(ctx context.Context, tx *sql.Tx, session string) (control.Session, error) {
	value, err := s.session(ctx, tx, session, true)
	if err == nil && value.View.Lifecycle != wire.LifecycleOpen {
		err = kernel.Fail(wire.ErrorCodeConflict, "session is not open")
	}
	if err == nil {
		err = s.liveLineage(ctx, tx, value, false)
	}
	return value, err
}

func scanInteraction(row scanner) (wire.Interaction, error) {
	var out wire.Interaction
	var revision int64
	var request, answer string
	err := row.Scan(&out.ID, &out.SessionID, &revision, &out.Status, &request, &answer)
	if err != nil {
		return out, notFound(err)
	}
	out.Revision = strconv.FormatInt(revision, 10)
	out.Request, err = decode[wire.InteractionCreate](request)
	if answer != "" {
		out.Answer = []byte(answer)
	}
	return out, err
}

const interactionColumns = "id,session_id,revision,status,request,answer"

func (s *Store) Interact(ctx context.Context, session string, request wire.InteractionCreate) (out wire.Interaction, err error) {
	if err = kernel.ValidateInteraction(request); err != nil {
		return
	}
	id := kernel.ID("interaction", session, request.RequestID)
	body := string(kernel.JSON(request))
	digest := kernel.Digest([]byte(body))
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.liveSession(ctx, tx, session); e != nil {
			return e
		}
		var prior string
		e := tx.QueryRowContext(ctx, s.q("SELECT request_hash FROM interactions WHERE id=?"), id).Scan(&prior)
		if e == nil && prior != digest {
			return kernel.Fail(wire.ErrorCodeConflict, "interaction key has different content")
		}
		if e != nil && !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		if e != nil {
			now, e := s.now(ctx, tx)
			if e != nil {
				return e
			}
			if request.ExpiresMs <= uint64(now) || request.ExpiresMs > uint64(now)+7*86400000 {
				return kernel.Fail(wire.ErrorCodeInvalid, "interaction expiry must be within seven days")
			}
			if request.Kind == wire.InteractionKindApproval {
				op, e := s.operation(ctx, tx, session, request.Subject)
				if e != nil {
					return e
				}
				if op.View.Status != wire.OperationStatusPrepared || op.View.RunID != request.RunID ||
					op.View.ArgumentsDigest != request.SubjectDigest {
					return kernel.Fail(wire.ErrorCodeConflict, "approval does not match the prepared operation and arguments")
				}
				var count int
				if e := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM interactions WHERE session_id=? AND subject=? AND status IN ('pending','answered')"), session, request.Subject).Scan(&count); e != nil {
					return e
				}
				if count != 0 {
					return kernel.Fail(wire.ErrorCodeConflict, "operation already has an approval interaction")
				}
			}
			_, e = tx.ExecContext(ctx, s.q(`INSERT INTO interactions(id,session_id,request,request_hash,expires_ms,subject,subject_digest) VALUES(?,?,?,?,?,?,?)`),
				id, session, body, digest, request.ExpiresMs, request.Subject, request.SubjectDigest)
			if e != nil {
				return e
			}
		}
		out, e = scanInteraction(tx.QueryRowContext(ctx, s.q("SELECT "+interactionColumns+" FROM interactions WHERE id=?"), id))
		return e
	})
	return
}

func (s *Store) Interactions(ctx context.Context, session string, limit int, after ...string) ([]wire.Interaction, error) {
	rows, err := s.pageRows(ctx, interactionColumns, "interactions", "session_id=? AND id>?", "id", s.bytes("request")+"+"+s.bytes("answer"), limit, session, control.After(after))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []wire.Interaction{}
	for rows.Next() {
		i, err := scanInteraction(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, i)
	}
	return out, rows.Err()
}

func (s *Store) Answer(ctx context.Context, session, id string, answer wire.InteractionAnswer) (out wire.Interaction, err error) {
	if err = kernel.ValidateAnswer(answer); err != nil {
		return
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.liveSession(ctx, tx, session)
		if e != nil {
			return e
		}
		out, err = scanInteraction(tx.QueryRowContext(ctx, s.q("SELECT "+interactionColumns+" FROM interactions WHERE id=? AND session_id=?"), id, session))
		if err != nil {
			return err
		}
		status := wire.InteractionStatusRejected
		if answer.Accept {
			status = wire.InteractionStatusAnswered
		}
		if out.Status != wire.InteractionStatusPending {
			var key string
			if err := tx.QueryRowContext(ctx, s.q("SELECT answer_key FROM interactions WHERE id=?"), id).Scan(&key); err != nil {
				return err
			}
			if key == answer.RequestID && out.Status == status && string(out.Answer) == string(answer.Answer) &&
				answer.SubjectDigest == out.Request.SubjectDigest {
				return nil
			}
			return kernel.Fail(wire.ErrorCodeConflict, "interaction was already resolved")
		}
		if out.Revision != answer.Revision || out.Request.SubjectDigest != answer.SubjectDigest {
			return kernel.Fail(wire.ErrorCodeConflict, "stale interaction revision or subject digest")
		}
		e = requireOne(tx.ExecContext(ctx, s.q("UPDATE interactions SET status=?,revision=revision+1,answer=?,answer_key=? WHERE id=? AND session_id=? AND revision=? AND status='pending' AND expires_ms>"+s.clock()),
			status, string(answer.Answer), answer.RequestID, id, session, answer.Revision))
		if e != nil {
			return e
		}
		if _, e = s.accept(ctx, tx, kernel.Principal{Tenant: current.Tenant, Subject: "kernel"}, session, wire.Input{RequestID: "answered:" + id + ":" + answer.RequestID,
			Data: wire.InputData{Kind: "journal", Name: kernel.Ptr("interaction.answered"), Data: kernel.JSON(map[string]string{"id": id, "subject": out.Request.Subject, "status": string(status)})}}); e != nil {
			return e
		}
		out, e = scanInteraction(tx.QueryRowContext(ctx, s.q("SELECT "+interactionColumns+" FROM interactions WHERE id=?"), id))
		return e
	})
	return
}

func scanConnection(row scanner) (wire.Connection, error) {
	var out wire.Connection
	var generation int64
	var resource, subject string
	err := row.Scan(&out.ID, &out.SessionID, &out.Template, &generation, &out.Status, &out.Ownership, &resource, &subject)
	out.Generation = strconv.FormatInt(generation, 10)
	if resource != "" {
		out.ResourceID = &resource
	}
	if subject != "" {
		out.ClaimSubject = &subject
	}
	return out, notFound(err)
}

const connectionColumns = "id,session_id,template,generation,status,ownership,resource_id,claim_subject"

func (s *Store) Attach(ctx context.Context, session string, request wire.ConnectionAttach) (out wire.Connection, err error) {
	if err = kernel.ValidateAttachment(request); err != nil {
		return
	}
	id := kernel.ID("connection", session, request.RequestID)
	hash := kernel.Digest(kernel.JSON(request))
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.liveSession(ctx, tx, session)
		if e != nil {
			return e
		}
		var prior string
		e = tx.QueryRowContext(ctx, s.q("SELECT request_hash FROM connections WHERE id=?"), id).Scan(&prior)
		if e == nil {
			if hash != prior {
				return kernel.Fail(wire.ErrorCodeConflict, "connection key has different arguments")
			}
			out, e = scanConnection(tx.QueryRowContext(ctx, s.q("SELECT "+connectionColumns+" FROM connections WHERE id=?"), id))
			return e
		}
		if !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		var template *wire.ConnectionTemplate
		for _, t := range current.View.Config.Connections {
			if t.Name == request.Template {
				template = &t
				break
			}
		}
		if template == nil {
			return kernel.Fail(wire.ErrorCodeForbidden, "connection template is not admitted")
		}
		var count int
		if err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM connections WHERE session_id=? AND status='active'"), session).Scan(&count); err != nil {
			return err
		}
		if count >= 32 {
			return kernel.Fail(wire.ErrorCodeOverloaded, "active connection limit reached")
		}
		var generation int64
		if err := tx.QueryRowContext(ctx, s.q("SELECT COALESCE(MAX(generation),0)+1 FROM connections WHERE session_id=? AND template=?"), session, request.Template).Scan(&generation); err != nil {
			return err
		}
		subject, e := s.authoritySubject(ctx, tx)
		if e != nil {
			return e
		}
		_, e = tx.ExecContext(ctx, s.q("INSERT INTO connections(id,session_id,template,generation,ownership,resource_id,request_hash,claim_subject) VALUES(?,?,?,?,?,?,?,?)"),
			id, session, request.Template, generation, template.Ownership, kernel.Value(request.ResourceID), hash, subject)
		if e != nil {
			return e
		}
		out, e = scanConnection(tx.QueryRowContext(ctx, s.q("SELECT "+connectionColumns+" FROM connections WHERE id=?"), id))
		return e
	})
	return
}

func (s *Store) Connections(ctx context.Context, session string, limit int, after ...string) ([]wire.Connection, error) {
	rows, err := s.pageRows(ctx, connectionColumns, "connections", "session_id=? AND id>?", "id", "6*"+s.bytes("resource_id"), limit, session, control.After(after))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []wire.Connection{}
	for rows.Next() {
		c, err := scanConnection(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

func (s *Store) RevokeConnection(ctx context.Context, session, id string) (out wire.Connection, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.liveSession(ctx, tx, session); e != nil {
			return e
		}
		if err := requireOne(tx.ExecContext(ctx, s.q("UPDATE connections SET status='revoked' WHERE id=? AND session_id=?"), id, session)); err != nil {
			return err
		}
		var e error
		out, e = scanConnection(tx.QueryRowContext(ctx, s.q("SELECT "+connectionColumns+" FROM connections WHERE id=?"), id))
		return e
	})
	return
}

func scanDocument(row scanner) (wire.Document, error) {
	var out wire.Document
	var revision int64
	var expires uint64
	var value string
	err := row.Scan(&out.Key, &revision, &value, &expires)
	out.Revision = strconv.FormatInt(revision, 10)
	out.Value = []byte(value)
	if expires != 0 {
		out.ExpiresMs = &expires
	}
	return out, notFound(err)
}

func (s *Store) Documents(ctx context.Context, session, scope string, limit int, after ...string) ([]wire.Document, error) {
	rows, err := s.pageRows(ctx, "key,revision,value,expires_ms", "documents", "session_id=? AND scope=? AND key>? AND (expires_ms=0 OR expires_ms>"+s.clock()+")",
		"key", s.bytes("value"), limit, session, scope, control.After(after))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []wire.Document{}
	for rows.Next() {
		d, err := scanDocument(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, d)
	}
	return out, rows.Err()
}

func (s *Store) PutDocument(ctx context.Context, session, scope, key string, update wire.DocumentUpdate) (out wire.Document, err error) {
	if !kernel.Identifier(key) || (scope != "memory" && scope != "state") || len(update.Value) > 65536 ||
		kernel.Value(update.ExpiresMs) > 1<<53-1 {
		return out, kernel.Fail(wire.ErrorCodeInvalid, "document fields exceed their admitted bounds")
	}
	hash := kernel.Digest(kernel.JSON(update))
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.liveSession(ctx, tx, session); e != nil {
			return e
		}
		var revision int64
		var priorKey, priorHash string
		e := tx.QueryRowContext(ctx, s.q("SELECT revision,request_key,request_hash FROM documents WHERE session_id=? AND scope=? AND key=?"), session, scope, key).Scan(&revision, &priorKey, &priorHash)
		if e != nil && !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		if e == nil && priorKey == update.RequestID {
			if priorHash != hash {
				return kernel.Fail(wire.ErrorCodeConflict, "document key has different arguments")
			}
		} else {
			expected, e := kernel.Sequence(update.Revision)
			if e != nil {
				return e
			}
			if expected != revision {
				return kernel.Fail(wire.ErrorCodeConflict, "document revision changed")
			}
			if e := s.documentQuota(ctx, tx, session, scope, key, update.Value); e != nil {
				return e
			}
			_, e = tx.ExecContext(ctx, s.q(`INSERT INTO documents(session_id,scope,key,revision,value,expires_ms,request_key,request_hash)
VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(session_id,scope,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,
expires_ms=excluded.expires_ms,request_key=excluded.request_key,request_hash=excluded.request_hash`),
				session, scope, key, revision+1, string(update.Value), kernel.Value(update.ExpiresMs), update.RequestID, hash)
			if e != nil {
				return e
			}
		}
		out, e = scanDocument(tx.QueryRowContext(ctx, s.q("SELECT key,revision,value,expires_ms FROM documents WHERE session_id=? AND scope=? AND key=?"), session, scope, key))
		return e
	})
	return
}

func (s *Store) DeleteDocument(ctx context.Context, session, scope, key string) (out wire.Document, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.liveSession(ctx, tx, session); e != nil {
			return e
		}
		var e error
		out, e = scanDocument(tx.QueryRowContext(ctx, s.q("DELETE FROM documents WHERE session_id=? AND scope=? AND key=? RETURNING key,revision,value,expires_ms"), session, scope, key))
		return e
	})
	return
}

func (s *Store) PutFile(ctx context.Context, session string, file wire.FileInfo) error {
	if len(file.Name) > 128 || len(file.MediaType) > 256 || len(file.Provenance) > 128 || file.Size > 16<<20 {
		return kernel.Fail(wire.ErrorCodeInvalid, "file metadata exceeds its admitted bounds")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.liveSession(ctx, tx, session); e != nil {
			return e
		}
		var count, size int64
		if e := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*),COALESCE(SUM(size),0) FROM files WHERE session_id=?"), session).Scan(&count, &size); e != nil {
			return e
		}
		var previousBytes int64
		existing := tx.QueryRowContext(ctx, s.q("SELECT size FROM files WHERE session_id=? AND name=?"), session, file.Name).Scan(&previousBytes)
		if existing != nil && !errors.Is(existing, sql.ErrNoRows) {
			return existing
		}
		if (errors.Is(existing, sql.ErrNoRows) && count >= 1024) || size-previousBytes+int64(file.Size) > 1024*1024*1024 {
			return kernel.Fail(wire.ErrorCodeOverloaded, "file quota reached")
		}
		_, err := tx.ExecContext(ctx, s.q(`INSERT INTO files(session_id,name,digest,size,media_type,provenance) VALUES(?,?,?,?,?,?)
ON CONFLICT(session_id,name) DO UPDATE SET digest=excluded.digest,size=excluded.size,media_type=excluded.media_type,provenance=excluded.provenance`),
			session, file.Name, file.Digest, file.Size, file.MediaType, file.Provenance)
		return err
	})
}

func (s *Store) Files(ctx context.Context, session string, limit int, after ...string) ([]wire.FileInfo, error) {
	rows, err := s.pageRows(ctx, "name,digest,size,media_type,provenance", "files", "session_id=? AND name>?", "name", "6*"+s.bytes("provenance"), limit, session, control.After(after))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []wire.FileInfo{}
	for rows.Next() {
		var f wire.FileInfo
		if err := rows.Scan(&f.Name, &f.Digest, &f.Size, &f.MediaType, &f.Provenance); err != nil {
			return nil, err
		}
		out = append(out, f)
	}
	return out, rows.Err()
}

func (s *Store) DeleteFile(ctx context.Context, session, name string) (out wire.FileInfo, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.liveSession(ctx, tx, session); e != nil {
			return e
		}
		return notFound(tx.QueryRowContext(ctx, s.q("DELETE FROM files WHERE session_id=? AND name=? RETURNING name,digest,size,media_type,provenance"), session, name).
			Scan(&out.Name, &out.Digest, &out.Size, &out.MediaType, &out.Provenance))
	})
	return
}

func (s *Store) Handoff(ctx context.Context, session string, request wire.Handoff) (out control.Session, err error) {
	hash := kernel.Digest(kernel.JSON(request))
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.session(ctx, tx, session, true)
		if e != nil {
			return e
		}
		var prior string
		e = tx.QueryRowContext(ctx, s.q("SELECT request_hash FROM harness_handoffs WHERE session_id=? AND request_key=?"), session, request.RequestID).Scan(&prior)
		if e == nil {
			if prior != hash {
				return kernel.Fail(wire.ErrorCodeConflict, "handoff request id has different arguments")
			}
			out = current
			return nil
		}
		if !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		if e = s.unfrozen(ctx, tx, session, false); e != nil {
			return e
		}
		if current.View.Lifecycle != wire.LifecycleOpen {
			return kernel.Fail(wire.ErrorCodeConflict, "session is not open")
		}
		if e = s.liveLineage(ctx, tx, current, false); e != nil {
			return e
		}
		if current.View.Revision != request.Revision {
			return kernel.Fail(wire.ErrorCodeConflict, "session revision changed")
		}
		admitted := false
		for _, harness := range current.View.Config.HarnessTransitions {
			if kernel.EqualJSON(harness, request.Harness) {
				admitted = true
			}
		}
		if !admitted {
			return kernel.Fail(wire.ErrorCodeForbidden, "harness transition is not admitted")
		}
		var count int
		if e := tx.QueryRowContext(ctx, s.q(`SELECT CASE WHEN EXISTS(
SELECT 1 FROM runs WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')) OR EXISTS(
SELECT 1 FROM commands WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event'))
THEN 1 ELSE 0 END`), session, session).Scan(&count); e != nil {
			return e
		}
		if count != 0 {
			return kernel.Fail(wire.ErrorCodeConflict, "harness handoff requires settled runs and accepted work")
		}
		capability := kernel.HarnessCapability(request.Harness, current.View.Config.Model.Provider)
		var liveOwner int
		if e = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM nodes WHERE id=? AND lease>"+s.clock()+" AND draining=0"), current.Owner.Node).Scan(&liveOwner); e != nil {
			return e
		}
		if liveOwner != 0 {
			if e = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM node_capabilities WHERE node_id=? AND capability=?"), current.Owner.Node, capability).Scan(&count); e != nil {
				return e
			}
			if count != 1 {
				return kernel.Fail(wire.ErrorCodeUnsupported, "live owner does not support the admitted harness transition; transfer to a compatible node first")
			}
		}
		if _, e = s.accept(ctx, tx, kernel.Principal{Tenant: current.Tenant, Subject: "kernel"}, session, wire.Input{
			RequestID: "handoff:" + kernel.ID(session, request.RequestID),
			Data: wire.InputData{Kind: "journal", Name: kernel.Ptr("harness.handed_off"), Data: kernel.JSON(map[string]any{
				"request_id": request.RequestID, "from": current.View.Config.Harness, "to": request.Harness, "prior_revision": request.Revision,
			})},
		}); e != nil {
			return e
		}
		current.View.Config.Harness = request.Harness
		if e := requireOne(tx.ExecContext(ctx, s.q("UPDATE sessions SET config=?,revision=revision+1 WHERE id=? AND revision=?"),
			string(kernel.JSON(current.View.Config)), session, request.Revision)); e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, s.q("DELETE FROM session_requirements WHERE session_id=? AND capability LIKE 'harness:%'"), session); e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, s.q("INSERT INTO session_requirements VALUES(?,?)"), session, capability); e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, s.q("INSERT INTO harness_handoffs VALUES(?,?,?)"), session, request.RequestID, hash); e != nil {
			return e
		}
		out, e = s.session(ctx, tx, session, false)
		return e
	})
	return
}

func (s *Store) Delete(ctx context.Context, session string) (out control.Session, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.session(ctx, tx, session, true)
		if e != nil {
			return e
		}
		if current.View.Lifecycle != wire.LifecycleDeleted {
			if e = s.cancelLineage(ctx, tx, current); e != nil {
				return e
			}
			if _, e := tx.ExecContext(ctx, s.q("UPDATE sessions SET lifecycle='deleted',generation=generation+1,revision=revision+1,ready=0,cleanup_ready=1,cleanup_ms="+s.clock()+" WHERE id=?"), session); e != nil {
				return e
			}
			for _, query := range []string{
				"UPDATE runs SET status='cancelled' WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')",
				"UPDATE commands SET status='cancelled' WHERE session_id=? AND status='accepted'",
				"UPDATE schedules SET status='cancelled' WHERE session_id=?",
				"UPDATE participants SET revoked=1 WHERE session_id=?",
				"UPDATE connections SET status='revoked' WHERE session_id=?",
				"UPDATE interactions SET status='cancelled' WHERE session_id=? AND status='pending'",
			} {
				if _, e := tx.ExecContext(ctx, s.q(query), session); e != nil {
					return e
				}
			}
			if _, e = tx.ExecContext(ctx, s.q(wakeParents), session); e != nil {
				return e
			}
		}
		out, e = s.session(ctx, tx, session, false)
		return e
	})
	return
}

func (s *Store) PublishCoverage(ctx context.Context, owner control.Owner, coverage control.Coverage) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.guard(ctx, tx, coverage.Session, owner, false)
		if e != nil {
			return e
		}
		if current.Generation != coverage.Generation {
			return kernel.Fail(wire.ErrorCodeConflict, "archive deletion generation changed")
		}
		_, e = tx.ExecContext(ctx, s.q(`INSERT INTO coverage(session_id,first_sequence,last_sequence,object_key,digest,bytes,generation)
VALUES(?,?,?,?,?,?,?) ON CONFLICT DO NOTHING`), coverage.Session, coverage.First, coverage.Last, coverage.Key, coverage.Digest, coverage.Bytes, coverage.Generation)
		if e != nil {
			return e
		}
		if len(coverage.Records) == 0 || len(coverage.Records) > 128 {
			return kernel.Fail(wire.ErrorCodeInvalid, "archive coverage requires its exact bounded membership")
		}
		prior := int64(0)
		for _, record := range coverage.Records {
			if record.Sequence <= prior || record.Sequence < coverage.First || record.Sequence > coverage.Last {
				return kernel.Fail(wire.ErrorCodeInvalid, "invalid archive membership order")
			}
			prior = record.Sequence
			if e = requireOne(tx.ExecContext(ctx, s.q("UPDATE allocations SET archive_key=? WHERE session_id=? AND sequence=? AND status='written' AND digest=?"), coverage.Key, coverage.Session, record.Sequence, record.Digest)); e != nil {
				return e
			}
			if _, e = tx.ExecContext(ctx, s.q(`UPDATE operations SET result_body='',
arguments=CASE WHEN EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=operations.session_id AND a.sequence=operations.intent_sequence AND a.archive_key<>'') THEN '' ELSE arguments END
WHERE session_id=? AND result_sequence=? AND result_digest=? AND status IN ('completed','failed')`), coverage.Session, record.Sequence, record.Digest); e != nil {
				return e
			}
			if _, e = tx.ExecContext(ctx, s.q("UPDATE operations SET arguments='' WHERE session_id=? AND intent_sequence=? AND intent_digest=? AND status IN ('completed','failed')"), coverage.Session, record.Sequence, record.Digest); e != nil {
				return e
			}
		}
		return nil
	})
}

func (s *Store) Coverage(ctx context.Context, session string, after int64, limit int) ([]control.Coverage, error) {
	rows, err := s.db.QueryContext(ctx, s.q(`SELECT c.session_id,c.first_sequence,c.last_sequence,c.object_key,c.digest,c.bytes,c.generation
FROM coverage c JOIN allocations a ON a.session_id=c.session_id AND a.archive_key=c.object_key
WHERE c.session_id=? AND a.sequence=? LIMIT ?`), session, after+1, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Coverage{}
	for rows.Next() {
		var c control.Coverage
		if err := rows.Scan(&c.Session, &c.First, &c.Last, &c.Key, &c.Digest, &c.Bytes, &c.Generation); err != nil {
			return nil, err
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

func (s *Store) PutPackage(ctx context.Context, tenant string, pkg wire.Package) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		_, err := tx.ExecContext(ctx, s.q("INSERT INTO packages(tenant,id,body) VALUES(?,?,?) ON CONFLICT(tenant,id) DO NOTHING"),
			tenant, pkg.ID, string(kernel.JSON(pkg)))
		return err
	})
}

func (s *Store) Package(ctx context.Context, tenant, id string) (wire.Package, error) {
	var body string
	if err := s.db.QueryRowContext(ctx, s.q("SELECT body FROM packages WHERE tenant=? AND id=?"), tenant, id).Scan(&body); err != nil {
		return wire.Package{}, notFound(err)
	}
	return decode[wire.Package](body)
}
