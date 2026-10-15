// Package history owns bounded local transcript storage and immutable archive
// publication. Control facts remain authoritative when transcript bytes are lost.
package history

import (
	"bytes"
	"compress/gzip"
	"container/list"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"hash/fnv"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"sync/atomic"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	_ "modernc.org/sqlite"
)

const MaxFrame = 256 << 10
const MaxBlock = 4 << 20
const MaxFrames = 128

type Archive interface {
	Put(context.Context, string, []byte) error
	Get(context.Context, string, int64) ([]byte, error)
	Delete(context.Context, string) error
}

type Config struct {
	Directory   string
	Shards      int
	CacheBytes  int64
	DiskBytes   int64
	DirtyBytes  int64
	Subscribers int
}

type Store struct {
	config         Config
	shards         []*sql.DB
	lockDB         *sql.DB
	lockTx         *sql.Tx
	cache          cache
	control        control.Store
	archive        Archive
	node           string
	mu             sync.Mutex
	watchers       map[string]map[chan struct{}]struct{}
	subscribers    int
	remoteReads    atomic.Int64
	hydrations     chan struct{}
	remote         func(context.Context, string, string, int64, string) (wire.Record, error)
	maintenance    sync.Mutex
	reserveCursors []string
	confirmCursors []confirmationCursor
}

type entry struct {
	key    string
	record wire.Record
	size   int64
}
type cache struct {
	sync.Mutex
	items map[string]*list.Element
	order *list.List
	bytes int64
	max   int64
}

func Open(ctx context.Context, config Config, authority control.Store, archive Archive, node string) (*Store, error) {
	if config.Shards < 1 || config.Shards > 32 || config.CacheBytes < 0 ||
		config.DiskBytes < 16<<20 || config.DirtyBytes < 1<<20 || config.DirtyBytes > config.DiskBytes {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid bounded history configuration")
	}
	if err := os.MkdirAll(config.Directory, 0700); err != nil {
		return nil, err
	}
	s := &Store{config: config, control: authority, archive: archive, node: node, watchers: map[string]map[chan struct{}]struct{}{}, hydrations: make(chan struct{}, 4)}
	s.reserveCursors = make([]string, config.Shards)
	s.confirmCursors = make([]confirmationCursor, config.Shards)
	s.cache = cache{items: map[string]*list.Element{}, order: list.New(), max: config.CacheBytes}
	var err error
	s.lockDB, err = sql.Open("sqlite", filepath.Join(config.Directory, "writer.sqlite")+"?_txlock=immediate")
	if err != nil {
		return nil, err
	}
	s.lockDB.SetMaxOpenConns(1)
	if _, err = s.lockDB.ExecContext(ctx, "CREATE TABLE IF NOT EXISTS writer(id INTEGER PRIMARY KEY)"); err == nil {
		s.lockTx, err = s.lockDB.BeginTx(ctx, nil)
	}
	if err != nil {
		s.Close()
		return nil, kernel.Fail(wire.ErrorCodeConflict, "history directory already has a writer")
	}
	for i := 0; i < config.Shards; i++ {
		path := filepath.Join(config.Directory, fmt.Sprintf("history-%02d.sqlite", i))
		db, err := sql.Open("sqlite", path+"?_pragma=journal_mode(WAL)&_pragma=synchronous(FULL)&_pragma=busy_timeout(5000)&_pragma=cache_size(-8192)&_pragma=mmap_size(0)&_pragma=wal_autocheckpoint(256)&_pragma=journal_size_limit(16777216)&_txlock=immediate")
		if err != nil {
			s.Close()
			return nil, err
		}
		db.SetMaxOpenConns(1)
		s.shards = append(s.shards, db)
		_, err = db.ExecContext(ctx, `PRAGMA auto_vacuum=INCREMENTAL;
CREATE TABLE IF NOT EXISTS records(session_id TEXT NOT NULL,sequence INTEGER NOT NULL,body BLOB NOT NULL,digest TEXT NOT NULL,size INTEGER NOT NULL,archived INTEGER NOT NULL DEFAULT 0,created_ms INTEGER NOT NULL,
original_writer TEXT NOT NULL DEFAULT '',original_epoch INTEGER NOT NULL DEFAULT 0,confirmed INTEGER NOT NULL DEFAULT 1,PRIMARY KEY(session_id,sequence));
CREATE INDEX IF NOT EXISTS dirty_records ON records(archived,created_ms,session_id,sequence);
CREATE TABLE IF NOT EXISTS reserves(id TEXT PRIMARY KEY,bytes INTEGER NOT NULL,session_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS totals(id INTEGER PRIMARY KEY,used INTEGER NOT NULL,dirty INTEGER NOT NULL,reserved INTEGER NOT NULL);
INSERT INTO totals VALUES(1,0,0,0) ON CONFLICT DO NOTHING;`)
		if err != nil {
			s.Close()
			return nil, err
		}
		var column int
		for name, declaration := range map[string]string{
			"original_writer": "TEXT NOT NULL DEFAULT ''",
			"original_epoch":  "INTEGER NOT NULL DEFAULT 0",
			"confirmed":       "INTEGER NOT NULL DEFAULT 1",
		} {
			if err = db.QueryRowContext(ctx, "SELECT COUNT(*) FROM pragma_table_info('records') WHERE name=?", name).Scan(&column); err == nil && column == 0 {
				_, err = db.ExecContext(ctx, "ALTER TABLE records ADD COLUMN "+name+" "+declaration)
			}
			if err != nil {
				s.Close()
				return nil, err
			}
		}
		if _, err = db.ExecContext(ctx, "CREATE INDEX IF NOT EXISTS record_confirmations ON records(confirmed,session_id,sequence)"); err != nil {
			s.Close()
			return nil, err
		}
		if err = db.QueryRowContext(ctx, "SELECT COUNT(*) FROM pragma_table_info('reserves') WHERE name='session_id'").Scan(&column); err == nil && column == 0 {
			_, err = db.ExecContext(ctx, "ALTER TABLE reserves ADD COLUMN session_id TEXT NOT NULL DEFAULT ''")
		}
		if err != nil {
			s.Close()
			return nil, err
		}
	}
	return s, nil
}

func (s *Store) Close() error {
	var err error
	for _, db := range s.shards {
		err = errors.Join(err, db.Close())
	}
	if s.lockTx != nil {
		_ = s.lockTx.Rollback()
	}
	if s.lockDB != nil {
		err = errors.Join(err, s.lockDB.Close())
	}
	return err
}

func (s *Store) shard(session string) *sql.DB {
	h := fnv.New32a()
	_, _ = h.Write([]byte(session))
	return s.shards[int(h.Sum32())%len(s.shards)]
}

func (s *Store) Reserve(ctx context.Context, session, operation string, bytes int64) error {
	db := s.shard(session)
	tx, err := db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	var old int64
	err = tx.QueryRowContext(ctx, "SELECT bytes FROM reserves WHERE id=?", operation).Scan(&old)
	if err == nil {
		if old != bytes {
			return kernel.Fail(wire.ErrorCodeConflict, "output reservation changed")
		}
		return nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	var used, dirty, reserved int64
	if err = tx.QueryRowContext(ctx, "SELECT used,dirty,reserved FROM totals WHERE id=1").Scan(&used, &dirty, &reserved); err != nil {
		return err
	}
	if used+reserved+bytes > s.config.DiskBytes/int64(len(s.shards)) || dirty+reserved+bytes > s.config.DirtyBytes/int64(len(s.shards)) {
		return kernel.Fail(wire.ErrorCodeOverloaded, "local transcript output reserve exhausted")
	}
	if _, err = tx.ExecContext(ctx, "INSERT INTO reserves(id,bytes,session_id) VALUES(?,?,?)", operation, bytes, session); err != nil {
		return err
	}
	if _, err = tx.ExecContext(ctx, "UPDATE totals SET reserved=reserved+? WHERE id=1", bytes); err != nil {
		return err
	}
	return tx.Commit()
}

func (s *Store) Release(ctx context.Context, session, operation string) error {
	tx, err := s.shard(session).BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	var size int64
	err = tx.QueryRowContext(ctx, "DELETE FROM reserves WHERE id=? RETURNING bytes", operation).Scan(&size)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if _, err = tx.ExecContext(ctx, "UPDATE totals SET reserved=reserved-? WHERE id=1", size); err != nil {
		return err
	}
	return tx.Commit()
}

func (s *Store) CheckAppend(ctx context.Context, record wire.Record, operation string) error {
	size := len(kernel.JSON(record))
	if size > MaxFrame {
		return kernel.Fail(wire.ErrorCodeInvalid, "record exceeds inline frame limit; store large data as a blob")
	}
	var used, dirty, reserved, claim int64
	err := s.shard(record.SessionID).QueryRowContext(ctx, `SELECT used,dirty,reserved,
COALESCE((SELECT bytes FROM reserves WHERE id=?),0) FROM totals WHERE id=1`, operation).Scan(&used, &dirty, &reserved, &claim)
	if err != nil {
		return err
	}
	cost := int64(size*3 + 1024)
	if used+reserved-claim+cost > s.config.DiskBytes/int64(len(s.shards)) ||
		dirty+reserved-claim+cost > s.config.DirtyBytes/int64(len(s.shards)) {
		return kernel.Fail(wire.ErrorCodeOverloaded, "transcript capacity is reserved for already admitted results")
	}
	return nil
}

func (s *Store) append(ctx context.Context, record wire.Record, operation string, hydrated bool, original control.Owner) (string, error) {
	body := kernel.JSON(record)
	if len(body) > MaxFrame {
		return "", kernel.Fail(wire.ErrorCodeInvalid, "record exceeds inline frame limit; store large data as a blob")
	}
	sequence, err := kernel.Sequence(record.Sequence)
	if err != nil {
		return "", err
	}
	digest := kernel.Digest(body)
	db := s.shard(record.SessionID)
	tx, err := db.BeginTx(ctx, nil)
	if err != nil {
		return "", err
	}
	defer tx.Rollback()
	var old string
	err = tx.QueryRowContext(ctx, "SELECT digest FROM records WHERE session_id=? AND sequence=?", record.SessionID, sequence).Scan(&old)
	if err == nil {
		if old != digest {
			return "", kernel.Fail(wire.ErrorCodeCorrupt, "record identity already has different bytes")
		}
		return digest, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return "", err
	}
	var used, dirty, reserved, claim int64
	if err = tx.QueryRowContext(ctx, "SELECT used,dirty,reserved FROM totals WHERE id=1").Scan(&used, &dirty, &reserved); err != nil {
		return "", err
	}
	if operation != "" {
		err = tx.QueryRowContext(ctx, "SELECT bytes FROM reserves WHERE id=?", operation).Scan(&claim)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return "", err
		}
	}
	cost := int64(len(body)*3 + 1024)
	if used+reserved-claim+cost > s.config.DiskBytes/int64(len(s.shards)) ||
		(!hydrated && dirty+reserved-claim+cost > s.config.DirtyBytes/int64(len(s.shards))) {
		return "", kernel.Fail(wire.ErrorCodeOverloaded, "transcript capacity is reserved for already admitted results")
	}
	archived := 0
	dirtyCost := cost
	if hydrated {
		archived = 1
		dirtyCost = 0
	}
	confirmed := 1
	if original.Node != "" {
		if !kernel.Identifier(original.Node) || original.Epoch < 1 {
			return "", kernel.Fail(wire.ErrorCodeInvalid, "local write has invalid original allocation authority")
		}
		confirmed = 0
	}
	if _, err = tx.ExecContext(ctx, `INSERT INTO records(session_id,sequence,body,digest,size,archived,created_ms,original_writer,original_epoch,confirmed)
VALUES(?,?,?,?,?,?,?,?,?,?)`, record.SessionID, sequence, body, digest, cost, archived, record.TimeMs, original.Node, original.Epoch, confirmed); err != nil {
		return "", err
	}
	if claim > 0 {
		if _, err = tx.ExecContext(ctx, "DELETE FROM reserves WHERE id=?", operation); err != nil {
			return "", err
		}
	}
	if _, err = tx.ExecContext(ctx, "UPDATE totals SET used=used+?,dirty=dirty+?,reserved=reserved-? WHERE id=1", cost, dirtyCost, claim); err != nil {
		return "", err
	}
	if err = tx.Commit(); err != nil {
		return "", kernel.Fail(wire.ErrorCodeUncertain, "local record commit outcome is uncertain")
	}
	s.cache.put(record)
	s.notify(record.SessionID)
	return digest, nil
}

func (s *Store) Append(ctx context.Context, record wire.Record, operation string, original control.Owner) (string, error) {
	return s.append(ctx, record, operation, false, original)
}

func (s *Store) SetRemoteReader(read func(context.Context, string, string, int64, string) (wire.Record, error)) {
	s.remote = read
}

func (s *Store) Local(ctx context.Context, session string, sequence int64, digest string) (wire.Record, error) {
	record, err := s.local(ctx, session, sequence)
	if err == nil && kernel.Digest(kernel.JSON(record)) != digest {
		return record, kernel.Fail(wire.ErrorCodeCorrupt, "requested local record digest does not match authoritative control")
	}
	return record, err
}

func cacheKey(session, sequence string) string { return session + ":" + sequence }

func (c *cache) put(record wire.Record) {
	key := cacheKey(record.SessionID, record.Sequence)
	size := int64(len(kernel.JSON(record))*2 + 256)
	if size > c.max {
		return
	}
	c.Lock()
	defer c.Unlock()
	if prior := c.items[key]; prior != nil {
		c.order.MoveToFront(prior)
		return
	}
	c.items[key] = c.order.PushFront(entry{key, record, size})
	c.bytes += size
	for c.bytes > c.max {
		last := c.order.Back()
		e := last.Value.(entry)
		delete(c.items, e.key)
		c.order.Remove(last)
		c.bytes -= e.size
	}
}

func (c *cache) get(session string, sequence int64) (wire.Record, bool) {
	c.Lock()
	defer c.Unlock()
	e := c.items[cacheKey(session, strconv.FormatInt(sequence, 10))]
	if e == nil {
		return wire.Record{}, false
	}
	c.order.MoveToFront(e)
	return e.Value.(entry).record, true
}

func (s *Store) local(ctx context.Context, session string, sequence int64) (wire.Record, error) {
	if record, ok := s.cache.get(session, sequence); ok {
		return record, nil
	}
	var body []byte
	var digest string
	err := s.shard(session).QueryRowContext(ctx, "SELECT CASE WHEN LENGTH(body)<=? THEN body ELSE NULL END,digest FROM records WHERE session_id=? AND sequence=?", MaxFrame, session, sequence).Scan(&body, &digest)
	if errors.Is(err, sql.ErrNoRows) {
		return wire.Record{}, kernel.Fail(wire.ErrorCodeUnavailable, "record body is not local")
	}
	if err != nil {
		return wire.Record{}, err
	}
	if kernel.Digest(body) != digest {
		return wire.Record{}, kernel.Fail(wire.ErrorCodeCorrupt, "local transcript digest mismatch")
	}
	var record wire.Record
	if err = json.Unmarshal(body, &record); err != nil || len(body) > MaxFrame || record.SessionID != session || record.Sequence != strconv.FormatInt(sequence, 10) {
		return record, kernel.Fail(wire.ErrorCodeCorrupt, "invalid local record")
	}
	s.cache.put(record)
	return record, nil
}

type block struct {
	Format  string        `json:"format"`
	Session string        `json:"session"`
	Records []wire.Record `json:"records"`
}
type manifest struct {
	Format    string   `json:"format"`
	Session   string   `json:"session"`
	Key       string   `json:"key"`
	Digest    string   `json:"digest"`
	RawBytes  int      `json:"raw_bytes"`
	Sequences []string `json:"sequences"`
}

func (s *Store) Get(ctx context.Context, session string, sequence int64, digest string) (wire.Record, error) {
	record, err := s.local(ctx, session, sequence)
	if err == nil {
		if digest != "" && kernel.Digest(kernel.JSON(record)) != digest {
			return record, kernel.Fail(wire.ErrorCodeCorrupt, "control and local record digests disagree")
		}
		return record, nil
	}
	if kernel.Response(err).Code != wire.ErrorCodeUnavailable {
		return record, err
	}
	select {
	case s.hydrations <- struct{}{}:
		defer func() { <-s.hydrations }()
	case <-ctx.Done():
		return record, ctx.Err()
	}
	if s.remote != nil {
		location, err := s.control.LocateRecord(ctx, session, sequence)
		if err != nil {
			return record, err
		}
		if location.Status == "written" && location.Node != s.node && location.Endpoint != "" {
			if digest != "" && location.Digest != digest {
				return record, kernel.Fail(wire.ErrorCodeCorrupt, "record location and expected digest disagree")
			}
			s.remoteReads.Add(1)
			routed, err := s.remote(ctx, location.Endpoint, session, sequence, location.Digest)
			if err == nil {
				if routed.SessionID != session || routed.Sequence != strconv.FormatInt(sequence, 10) || kernel.Digest(kernel.JSON(routed)) != location.Digest {
					return record, kernel.Fail(wire.ErrorCodeCorrupt, "routed record identity or digest mismatch")
				}
				// A remote L2 copy is not verified archive coverage.
				if location.CurrentOwner == s.node {
					if _, err = s.append(ctx, routed, "", false, control.Owner{}); err != nil && kernel.Response(err).Code != wire.ErrorCodeOverloaded {
						return record, err
					}
				}
				s.cache.put(routed)
				return routed, nil
			}
			if kernel.Response(err).Code == wire.ErrorCodeCorrupt {
				return record, err
			}
		}
	}
	return s.archived(ctx, session, sequence, digest)
}

func (s *Store) Archived(ctx context.Context, session string, sequence int64, digest string) (wire.Record, error) {
	select {
	case s.hydrations <- struct{}{}:
		defer func() { <-s.hydrations }()
	case <-ctx.Done():
		return wire.Record{}, ctx.Err()
	}
	return s.archived(ctx, session, sequence, digest)
}

func (s *Store) archived(ctx context.Context, session string, sequence int64, digest string) (wire.Record, error) {
	var record wire.Record
	coverage, err := s.control.Coverage(ctx, session, sequence-1, 32)
	if err != nil {
		return record, err
	}
	for _, c := range coverage {
		if c.First > sequence || c.Last < sequence {
			continue
		}
		s.remoteReads.Add(1)
		metadata, err := s.archive.Get(ctx, c.Key, 65536)
		if err != nil {
			return record, err
		}
		if kernel.Digest(metadata) != c.Digest {
			return record, kernel.Fail(wire.ErrorCodeCorrupt, "archive manifest digest mismatch")
		}
		var m manifest
		if err = json.Unmarshal(metadata, &m); err != nil || m.Format != "brain-transcript-manifest/1" || m.Session != session ||
			m.RawBytes > MaxBlock || len(m.Sequences) > MaxFrames {
			return record, kernel.Fail(wire.ErrorCodeCorrupt, "invalid archive manifest")
		}
		compressed, err := s.archive.Get(ctx, m.Key, MaxBlock+65536)
		if err != nil {
			return record, err
		}
		if kernel.Digest(compressed) != m.Digest {
			return record, kernel.Fail(wire.ErrorCodeCorrupt, "archive block digest mismatch")
		}
		reader, err := gzip.NewReader(bytes.NewReader(compressed))
		if err != nil {
			return record, kernel.Fail(wire.ErrorCodeCorrupt, "invalid archive compression")
		}
		body, readErr := io.ReadAll(io.LimitReader(reader, MaxBlock+1))
		closeErr := reader.Close()
		if readErr != nil || closeErr != nil || len(body) != m.RawBytes || len(body) > MaxBlock {
			return record, kernel.Fail(wire.ErrorCodeCorrupt, "invalid archive decoded length")
		}
		var b block
		if err = json.Unmarshal(body, &b); err != nil || b.Format != "brain-transcript/1" || b.Session != session || len(b.Records) != len(m.Sequences) {
			return record, kernel.Fail(wire.ErrorCodeCorrupt, "invalid archive records")
		}
		for i, r := range b.Records {
			if r.SessionID != session || r.Sequence != m.Sequences[i] || len(kernel.JSON(r)) > MaxFrame {
				return record, kernel.Fail(wire.ErrorCodeCorrupt, "archive record identity mismatch")
			}
			if r.Sequence == strconv.FormatInt(sequence, 10) {
				if digest != "" && kernel.Digest(kernel.JSON(r)) != digest {
					return record, kernel.Fail(wire.ErrorCodeCorrupt, "archive record digest disagrees with control")
				}
				_, err = s.append(ctx, r, "", true, control.Owner{})
				if err != nil && kernel.Response(err).Code != wire.ErrorCodeOverloaded {
					return record, err
				}
				return r, nil
			}
		}
	}
	return record, kernel.Fail(wire.ErrorCodeUnavailable, "written record body is unavailable; it was not an unused sequence")
}

func (s *Store) Page(ctx context.Context, session string, after int64, limit int) (wire.RecordPage, error) {
	page := wire.RecordPage{Records: []wire.Record{}, Gaps: []wire.Gap{}, Next: strconv.FormatInt(after, 10)}
	allocations, high, err := s.control.Allocations(ctx, session, after, limit)
	if err != nil {
		return page, err
	}
	page.AllocatedThrough = strconv.FormatInt(high, 10)
	bytes := 0
	for _, a := range allocations {
		if bytes+int(a.Bytes) > 1<<20 && len(page.Records) > 0 {
			break
		}
		seq := strconv.FormatInt(a.Sequence, 10)
		page.Next = seq
		if a.Status != "written" {
			kind := wire.GapKindReserved
			if a.Status == "unused" {
				kind = wire.GapKindUnused
			}
			if a.Status == "unresolved" {
				kind = wire.GapKindUnresolved
			}
			page.Gaps = append(page.Gaps, wire.Gap{First: seq, Last: seq, Kind: kind})
			continue
		}
		record, err := s.Get(ctx, session, a.Sequence, a.Digest)
		if err != nil {
			if kernel.Response(err).Code != wire.ErrorCodeUnavailable {
				return page, err
			}
			page.Gaps = append(page.Gaps, wire.Gap{First: seq, Last: seq, Kind: wire.GapKindUnavailable})
			continue
		}
		page.Records = append(page.Records, record)
		bytes += len(kernel.JSON(record))
	}
	next, _ := strconv.ParseInt(page.Next, 10, 64)
	page.More = next < high
	return page, nil
}

func (s *Store) Context(ctx context.Context, session string, through int64) ([]wire.Record, error) {
	return s.context(ctx, session, through, s.Get)
}

func (s *Store) ArchivedContext(ctx context.Context, session string, through int64) ([]wire.Record, error) {
	return s.context(ctx, session, through, s.Archived)
}

func (s *Store) context(ctx context.Context, session string, through int64, read func(context.Context, string, int64, string) (wire.Record, error)) ([]wire.Record, error) {
	allocations, _, err := s.control.Allocations(ctx, session, max(0, through-64), 64)
	if err != nil {
		return nil, err
	}
	selected := []control.Allocation{}
	bytes := int64(0)
	for i := len(allocations) - 1; i >= 0; i-- {
		a := allocations[i]
		if a.Sequence > through || a.Status == "unused" {
			continue
		}
		if a.Status != "written" {
			return nil, kernel.Fail(wire.ErrorCodeUnavailable, "selected context contains unresolved record positions")
		}
		if bytes+a.Bytes > 512<<10 {
			break
		}
		selected = append(selected, a)
		bytes += a.Bytes
	}
	records := make([]wire.Record, 0, len(selected))
	for i := len(selected) - 1; i >= 0; i-- {
		a := selected[i]
		record, err := read(ctx, session, a.Sequence, a.Digest)
		if err != nil {
			return nil, err
		}
		records = append(records, record)
	}
	return records, nil
}

func (s *Store) Flush(ctx context.Context, session string, owner control.Owner, generation int64) error {
	if s.archive == nil {
		return kernel.Fail(wire.ErrorCodeUnavailable, "archive is not configured")
	}
	if err := s.ReconcileSession(ctx, session); err != nil {
		return err
	}
	db := s.shard(session)
	rows, err := db.QueryContext(ctx, "SELECT body FROM records WHERE session_id=? AND archived=0 ORDER BY sequence LIMIT ?", session, MaxFrames)
	if err != nil {
		return err
	}
	b := block{Format: "brain-transcript/1", Session: session, Records: []wire.Record{}}
	total := 0
	for rows.Next() {
		var raw []byte
		if err = rows.Scan(&raw); err != nil {
			rows.Close()
			return err
		}
		if total+len(raw)+4096 > MaxBlock {
			break
		}
		var record wire.Record
		if err = json.Unmarshal(raw, &record); err != nil {
			rows.Close()
			return kernel.Fail(wire.ErrorCodeCorrupt, "invalid archive input")
		}
		b.Records = append(b.Records, record)
		total += len(raw)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	if len(b.Records) == 0 {
		return nil
	}
	body := kernel.JSON(b)
	if len(body) > MaxBlock {
		return kernel.Fail(wire.ErrorCodeInvalid, "archive raw block bound exceeded")
	}
	var compressed bytes.Buffer
	writer := gzip.NewWriter(&compressed)
	if _, err = writer.Write(body); err != nil {
		return err
	}
	if err = writer.Close(); err != nil {
		return err
	}
	digest := kernel.Digest(compressed.Bytes())
	key := "transcript/" + session + "/" + digest + ".gz"
	if err = s.archive.Put(ctx, key, compressed.Bytes()); err != nil {
		return err
	}
	m := manifest{Format: "brain-transcript-manifest/1", Session: session, Key: key, Digest: digest, RawBytes: len(body), Sequences: []string{}}
	for _, record := range b.Records {
		m.Sequences = append(m.Sequences, record.Sequence)
	}
	metadata := kernel.JSON(m)
	manifestDigest := kernel.Digest(metadata)
	manifestKey := "transcript/" + session + "/" + manifestDigest + ".json"
	if err = s.archive.Put(ctx, manifestKey, metadata); err != nil {
		return err
	}
	first, _ := kernel.Sequence(b.Records[0].Sequence)
	last, _ := kernel.Sequence(b.Records[len(b.Records)-1].Sequence)
	members := []control.Allocation{}
	for _, record := range b.Records {
		seq, _ := kernel.Sequence(record.Sequence)
		members = append(members, control.Allocation{Sequence: seq, Digest: kernel.Digest(kernel.JSON(record))})
	}
	if err = s.control.PublishCoverage(ctx, owner, control.Coverage{Session: session, First: first, Last: last, Key: manifestKey, Digest: manifestDigest, Bytes: int64(len(body)), Generation: generation, Records: members}); err != nil {
		return err
	}
	tx, err := db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	var cost int64
	for _, member := range members {
		var changed int64
		err = tx.QueryRowContext(ctx, "UPDATE records SET archived=1 WHERE session_id=? AND sequence=? AND digest=? AND archived=0 RETURNING size", session, member.Sequence, member.Digest).Scan(&changed)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return err
		}
		cost += changed
	}
	if _, err = tx.ExecContext(ctx, "UPDATE totals SET dirty=dirty-? WHERE id=1", cost); err != nil {
		return err
	}
	return tx.Commit()
}

func (s *Store) Maintain(ctx context.Context) error {
	s.maintenance.Lock()
	defer s.maintenance.Unlock()
	for index, db := range s.shards {
		if err := s.reconcilePending(ctx, db, index); err != nil {
			return err
		}
		if err := s.reconcileReserves(ctx, db, index); err != nil {
			return err
		}
		rows, err := db.QueryContext(ctx, "SELECT session_id FROM records WHERE archived=0 GROUP BY session_id ORDER BY MIN(created_ms) LIMIT 16")
		if err != nil {
			return err
		}
		sessions := []string{}
		for rows.Next() {
			var session string
			if err = rows.Scan(&session); err != nil {
				rows.Close()
				return err
			}
			sessions = append(sessions, session)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return err
		}
		for _, session := range sessions {
			current, err := s.control.Session(ctx, "", session)
			if err != nil {
				continue
			}
			if current.Owner.Node != s.node {
				current.Owner, err = s.control.ClaimOwner(ctx, session, s.node)
				if err != nil {
					continue
				}
			}
			if err = s.Flush(ctx, session, current.Owner, current.Generation); err != nil {
				return err
			}
		}
		tx, err := db.BeginTx(ctx, nil)
		if err != nil {
			return err
		}
		var used int64
		if err = tx.QueryRowContext(ctx, "SELECT used FROM totals WHERE id=1").Scan(&used); err == nil && used > s.config.DiskBytes/int64(len(s.shards))*3/4 {
			var removed int64
			err = tx.QueryRowContext(ctx, "SELECT COALESCE(SUM(size),0) FROM (SELECT size FROM records WHERE archived=1 ORDER BY created_ms LIMIT 256)").Scan(&removed)
			if err == nil {
				_, err = tx.ExecContext(ctx, "DELETE FROM records WHERE rowid IN (SELECT rowid FROM records WHERE archived=1 ORDER BY created_ms LIMIT 256)")
			}
			if err == nil {
				_, err = tx.ExecContext(ctx, "UPDATE totals SET used=used-? WHERE id=1", removed)
			}
		}
		if err != nil {
			tx.Rollback()
			return err
		}
		if err = tx.Commit(); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "PRAGMA wal_checkpoint(PASSIVE); PRAGMA incremental_vacuum(128);"); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) reconcileReserves(ctx context.Context, db *sql.DB, index int) error {
	rows, err := db.QueryContext(ctx, "SELECT id,session_id FROM reserves WHERE id>? AND session_id<>'' ORDER BY id LIMIT 128", s.reserveCursors[index])
	if err != nil {
		return err
	}
	type reservation struct{ id, session string }
	pending := []reservation{}
	for rows.Next() {
		var value reservation
		if err = rows.Scan(&value.id, &value.session); err != nil {
			rows.Close()
			return err
		}
		pending = append(pending, value)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	if len(pending) == 0 {
		s.reserveCursors[index] = ""
	}
	for _, value := range pending {
		s.reserveCursors[index] = value.id
		operation, err := s.control.Operation(ctx, value.session, value.id)
		if kernel.Response(err).Code == wire.ErrorCodeNotFound {
			continue
		}
		if err != nil {
			return err
		}
		if operation.ResultSequence > 0 && (operation.View.Status == wire.OperationStatusCompleted || operation.View.Status == wire.OperationStatusFailed) {
			if err = s.Release(ctx, value.session, value.id); err != nil {
				return err
			}
		}
	}
	return nil
}

func (s *Store) Stats(ctx context.Context) (cache, dirty, reserved, oldest int64, err error) {
	s.cache.Lock()
	cache = s.cache.bytes
	s.cache.Unlock()
	for _, db := range s.shards {
		var d, r, age int64
		if err = db.QueryRowContext(ctx, "SELECT dirty,reserved FROM totals WHERE id=1").Scan(&d, &r); err != nil {
			return
		}
		if err = db.QueryRowContext(ctx, "SELECT COALESCE(MIN(created_ms),0) FROM records WHERE archived=0").Scan(&age); err != nil {
			return
		}
		dirty += d
		reserved += r
		if age != 0 && (oldest == 0 || age < oldest) {
			oldest = age
		}
	}
	if oldest != 0 {
		oldest = time.Now().UnixMilli() - oldest
	}
	return
}

func (s *Store) Subscribe(session string) (<-chan struct{}, func(), error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.subscribers >= s.config.Subscribers {
		return nil, nil, kernel.Fail(wire.ErrorCodeOverloaded, "stream subscriber limit reached")
	}
	c := make(chan struct{}, 1)
	if s.watchers[session] == nil {
		s.watchers[session] = map[chan struct{}]struct{}{}
	}
	s.watchers[session][c] = struct{}{}
	s.subscribers++
	var once sync.Once
	return c, func() {
		once.Do(func() {
			s.mu.Lock()
			defer s.mu.Unlock()
			delete(s.watchers[session], c)
			if len(s.watchers[session]) == 0 {
				delete(s.watchers, session)
			}
			s.subscribers--
		})
	}, nil
}

func (s *Store) notify(session string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for c := range s.watchers[session] {
		select {
		case c <- struct{}{}:
		default:
		}
	}
}
