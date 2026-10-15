package store

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"sync"

	"github.com/aexhq/brain/internal/core"
)

const MaxRecord = 4 << 20

// Log serializes append and subscriber registration, never network I/O.
type Log struct {
	mu       sync.Mutex
	f        *os.File
	size     int64
	broken   error
	subs     map[*Subscription]struct{}
	maxBytes int64
}

type Subscription struct {
	Events   chan core.Event
	Done     chan struct{}
	bytes    int64
	maxBytes int64
	err      error
	log      *Log
}

func (s *Subscription) Close()     { s.log.mu.Lock(); defer s.log.mu.Unlock(); s.log.remove(s, nil) }
func (s *Subscription) Err() error { s.log.mu.Lock(); defer s.log.mu.Unlock(); return s.err }
func (s *Subscription) Consumed(e core.Event) {
	s.log.mu.Lock()
	defer s.log.mu.Unlock()
	s.bytes -= int64(len(e.Data) + 256)
}

type cursor struct {
	Offset int64  `json:"o"`
	ID     string `json:"i"`
}

func cursorFor(offset int64, id string) string {
	b, _ := json.Marshal(cursor{offset, id})
	return base64.RawURLEncoding.EncodeToString(b)
}

// OpenLog drops only an incomplete final line. A complete corrupt record fails.
func OpenLog(path string, maxBytes int64) (*Log, error) {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		return nil, err
	}
	l := &Log{f: f, subs: make(map[*Subscription]struct{}), maxBytes: maxBytes}
	r := bufio.NewReaderSize(f, 64<<10)
	for {
		b, err := readLine(r)
		if errors.Is(err, io.EOF) {
			if len(b) > 0 {
				if e := f.Truncate(l.size); e != nil {
					f.Close()
					return nil, e
				}
			}
			break
		}
		if err != nil {
			f.Close()
			return nil, err
		}
		if _, err := decodeEvent(b); err != nil {
			f.Close()
			return nil, fmt.Errorf("corrupt event at byte %d: %w", l.size, err)
		}
		l.size += int64(len(b))
	}
	if _, err := f.Seek(l.size, io.SeekStart); err != nil {
		f.Close()
		return nil, err
	}
	return l, nil
}

func readLine(r *bufio.Reader) ([]byte, error) {
	var b []byte
	for {
		part, err := r.ReadSlice('\n')
		if len(b)+len(part) > MaxRecord {
			return nil, fmt.Errorf("event exceeds %d bytes", MaxRecord)
		}
		b = append(b, part...)
		if errors.Is(err, bufio.ErrBufferFull) {
			continue
		}
		return b, err
	}
}

func decodeEvent(b []byte) (core.Event, error) {
	var e core.Event
	d := json.NewDecoder(bytes.NewReader(b))
	d.DisallowUnknownFields()
	if err := d.Decode(&e); err != nil {
		return e, err
	}
	if !core.ValidID(e.ID) || e.Type == "" || e.Time.IsZero() || len(e.Data) == 0 || e.Cursor != "" {
		return e, errors.New("invalid retained event")
	}
	if d.Decode(new(any)) != io.EOF {
		return e, errors.New("trailing event data")
	}
	return e, nil
}

func (l *Log) Size() int64 { l.mu.Lock(); defer l.mu.Unlock(); return l.size }
func (l *Log) Append(e core.Event) (core.Event, error) {
	e.Cursor = ""
	b, err := json.Marshal(e)
	if err != nil {
		return e, err
	}
	b = append(b, '\n')
	if len(b) > MaxRecord {
		return e, core.E(core.Overloaded, "event exceeds record limit")
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.broken != nil {
		return e, l.broken
	}
	if l.maxBytes > 0 && l.size+int64(len(b)) > l.maxBytes {
		return e, core.E(core.Overloaded, "session log limit reached")
	}
	n, err := l.f.Write(b)
	if err != nil || n != len(b) {
		if err == nil {
			err = io.ErrShortWrite
		}
		l.broken = err
		return e, err
	}
	e.Cursor = cursorFor(l.size, e.ID)
	l.size += int64(n)
	for s := range l.subs {
		sz := int64(len(e.Data) + 256)
		if s.bytes+sz > s.maxBytes {
			l.remove(s, core.E(core.Overloaded, "subscriber byte limit reached"))
			continue
		}
		select {
		case s.Events <- e:
			s.bytes += sz
		default:
			l.remove(s, core.E(core.Overloaded, "subscriber queue full"))
		}
	}
	return e, nil
}

func (l *Log) remove(s *Subscription, err error) {
	if _, ok := l.subs[s]; !ok {
		return
	}
	delete(l.subs, s)
	s.err = err
	close(s.Done)
}

// Subscribe captures the immutable replay boundary and registers live delivery
// under the append lock. Replay below that boundary can then happen unlocked.
func (l *Log) Subscribe(count int, bytes int64) (*Subscription, int64) {
	l.mu.Lock()
	defer l.mu.Unlock()
	s := &Subscription{Events: make(chan core.Event, count), Done: make(chan struct{}), maxBytes: bytes, log: l}
	l.subs[s] = struct{}{}
	return s, l.size
}

func (l *Log) Read(after string, limit int, until int64) ([]core.Event, error) {
	if limit < 1 || limit > 1000 {
		return nil, core.E(core.Invalid, "invalid history limit")
	}
	if until < 0 {
		until = l.Size()
	}
	var offset int64
	if after != "" {
		b, err := base64.RawURLEncoding.DecodeString(after)
		var c cursor
		if err != nil || json.Unmarshal(b, &c) != nil || c.Offset < 0 || c.Offset >= until || !core.ValidID(c.ID) {
			return nil, core.E(core.CursorUnavailable, "cursor is outside retained history")
		}
		r := bufio.NewReader(io.NewSectionReader(l.f, c.Offset, until-c.Offset))
		line, err := readLine(r)
		e, decErr := decodeEvent(line)
		if err != nil || decErr != nil || e.ID != c.ID {
			return nil, core.E(core.CursorUnavailable, "cursor is outside retained history")
		}
		offset = c.Offset + int64(len(line))
	}
	r := bufio.NewReaderSize(io.NewSectionReader(l.f, offset, until-offset), 64<<10)
	events := make([]core.Event, 0, limit)
	for len(events) < limit {
		line, err := readLine(r)
		if errors.Is(err, io.EOF) && len(line) == 0 {
			break
		}
		if err != nil {
			return nil, err
		}
		e, err := decodeEvent(line)
		if err != nil {
			return nil, err
		}
		e.Cursor = cursorFor(offset, e.ID)
		events = append(events, e)
		offset += int64(len(line))
	}
	return events, nil
}

func (l *Log) Close() error {
	l.mu.Lock()
	defer l.mu.Unlock()
	for s := range l.subs {
		l.remove(s, errors.New("log closed"))
	}
	if l.broken == nil {
		l.broken = errors.New("log closed")
	}
	return l.f.Close()
}
