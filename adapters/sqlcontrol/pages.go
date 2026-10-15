package sqlcontrol

import (
	"context"
	"database/sql"
	"strings"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) bytes(column string) string {
	if s.postgres {
		return "OCTET_LENGTH(" + column + ")"
	}
	return "LENGTH(CAST(" + column + " AS BLOB))"
}

func (s *Store) pageRows(ctx context.Context, columns, source, predicate, key, size string, limit int, arguments ...any) (*sql.Rows, error) {
	if limit < 1 || limit > 1024 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "page limit must be between 1 and 1024")
	}
	projection := strings.NewReplacer("r.", "", "c.", "").Replace(columns)
	query := `WITH candidates AS (SELECT ` + columns + "," + key + " AS page_key," + size + `+4096 AS body_bytes FROM ` + source +
		" WHERE " + predicate + " ORDER BY " + key + ` LIMIT ?),
bounded AS (SELECT candidates.*,SUM(body_bytes) OVER(ORDER BY page_key ROWS UNBOUNDED PRECEDING) AS page_bytes FROM candidates)
SELECT ` + projection + ` FROM bounded WHERE page_bytes<=? ORDER BY page_key`
	return s.db.QueryContext(ctx, s.q(query), append(arguments, limit, 2<<20)...)
}

const runnablePredicate = `r.status IN ('queued','active') OR (r.status='waiting' AND (
(r.wait_kind='operation' AND EXISTS(SELECT 1 FROM operations o WHERE o.id=r.wait_id AND o.status IN ('completed','failed','unknown')))
OR (r.wait_kind='dispatch' AND EXISTS(SELECT 1 FROM operations o WHERE o.id=r.wait_id AND
(o.status<>'prepared' OR o.approval=0 OR EXISTS(SELECT 1 FROM interactions i
WHERE i.session_id=o.session_id AND i.subject=o.id AND i.subject_digest=o.arguments_digest AND i.status='answered'))))
OR (r.wait_kind='interaction' AND EXISTS(SELECT 1 FROM interactions i WHERE i.id=r.wait_id AND i.status<>'pending'))
OR (r.wait_kind='child' AND NOT EXISTS(SELECT 1 FROM runs child WHERE child.session_id=r.wait_id AND child.status IN ('queued','active','waiting','paused'))
AND NOT EXISTS(SELECT 1 FROM commands child WHERE child.session_id=r.wait_id AND child.status='accepted' AND child.kind IN ('message','action','event')))
OR (` + childrenReady + `)))`

func (s *Store) runBytes() string {
	return s.bytes("r.checkpoint") + "+" + s.bytes("r.decision") + "+" + s.bytes("r.error") + "+" + s.bytes("c.payload")
}

func (s *Store) RunnableRuns(ctx context.Context, session string, limit int) ([]control.Run, error) {
	rows, err := s.pageRows(ctx, runColumns, "runs r JOIN commands c ON c.id=r.command_id", "r.session_id=? AND ("+runnablePredicate+")",
		"r.id", s.runBytes(), limit, session)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Run{}
	for rows.Next() {
		run, err := scanRun(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, run)
	}
	return out, rows.Err()
}

func (s *Store) operationBytes() string {
	return s.bytes("arguments") + "+" + s.bytes("result_body") + "+" + s.bytes("usage") + "+6*" + s.bytes("error")
}

func (s *Store) PendingResults(ctx context.Context, session string, limit int) ([]control.Operation, error) {
	rows, err := s.pageRows(ctx, operationColumns, "operations", "session_id=? AND result_sequence=0 AND status IN ('completed','failed','unknown')",
		"id", s.operationBytes(), limit, session)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Operation{}
	for rows.Next() {
		value, err := scanOperation(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, value)
	}
	return out, rows.Err()
}

func (s *Store) ChildState(ctx context.Context, session string) (out control.ChildState, err error) {
	err = s.db.QueryRowContext(ctx, s.q(`SELECT
(SELECT COUNT(*) FROM runs WHERE session_id=? AND status IN ('queued','active','waiting','paused'))+
(SELECT COUNT(*) FROM commands WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event')),
(SELECT COUNT(*) FROM runs WHERE session_id=? AND status IN ('failed','cancelled','blocked'))+
(SELECT COUNT(*) FROM commands WHERE session_id=? AND status IN ('failed','cancelled') AND kind IN ('message','action','event') AND run_id=''),
(SELECT COUNT(*) FROM runs WHERE session_id=? AND status='completed')`),
		session, session, session, session, session).Scan(&out.Pending, &out.Failed, &out.Completed)
	return
}

func (s *Store) Interaction(ctx context.Context, session, id string) (wire.Interaction, error) {
	return scanInteraction(s.db.QueryRowContext(ctx, s.q("SELECT "+interactionColumns+" FROM interactions WHERE session_id=? AND id=?"), session, id))
}

func (s *Store) ActiveConnection(ctx context.Context, session, template string) (wire.Connection, error) {
	return scanConnection(s.db.QueryRowContext(ctx, s.q("SELECT "+connectionColumns+" FROM connections WHERE session_id=? AND template=? AND status='active' ORDER BY generation DESC,id LIMIT 1"), session, template))
}

func (s *Store) File(ctx context.Context, session, name string) (out wire.FileInfo, err error) {
	err = s.db.QueryRowContext(ctx, s.q("SELECT name,digest,size,media_type,provenance FROM files WHERE session_id=? AND name=?"), session, name).
		Scan(&out.Name, &out.Digest, &out.Size, &out.MediaType, &out.Provenance)
	return out, notFound(err)
}
