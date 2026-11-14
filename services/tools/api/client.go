package api

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
	"io"
	"net/http"
	"strings"
	"time"
)

type Client struct {
	Owner      string
	Epoch      uint64
	URL, Token string
	HTTP       *http.Client
}

func (c *Client) Do(ctx context.Context, in Request, progress func(json.RawMessage)) (Frame, error) {
	var out Frame
	b, e := json.Marshal(in)
	if e != nil {
		return out, e
	}
	r, e := http.NewRequestWithContext(ctx, "POST", strings.TrimRight(c.URL, "/")+"/rpc", bytes.NewReader(b))
	if e != nil {
		return out, e
	}
	r.Header.Set("Authorization", "Bearer "+c.Token)
	r.Header.Set("Content-Type", "application/json")
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(r.Header))
	h := c.HTTP
	if h == nil {
		h = &http.Client{}
	}
	response, e := h.Do(r)
	if e != nil {
		return out, e
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return out, errors.New("tools service request failed")
	}
	s := bufio.NewScanner(response.Body)
	s.Buffer(make([]byte, 4096), 4<<20)
	for s.Scan() {
		var f Frame
		if e = json.Unmarshal(s.Bytes(), &f); e != nil {
			return out, e
		}
		if f.Error != nil {
			return out, f.Error
		}
		if len(f.Progress) > 0 {
			if progress != nil {
				progress(f.Progress)
			}
			continue
		}
		if f.Done {
			return f, nil
		}
	}
	if e = s.Err(); e == nil {
		e = io.ErrUnexpectedEOF
	}
	return out, e
}
func input(op string, s core.Session) Request {
	return Request{Operation: op, Tenant: s.Tenant, Session: s, Credentials: s.Credentials}
}
func (c *Client) Bind(ctx context.Context, s *core.Session) error {
	f, e := c.Do(ctx, input("bind", *s), nil)
	if e != nil {
		return e
	}
	if f.Session == nil {
		return errors.New("tools omitted binding response")
	}
	s.Descriptors = f.Session.Descriptors
	return nil
}
func (c *Client) Describe(ctx context.Context, s core.Session) ([]core.Tool, error) {
	f, e := c.Do(ctx, input("describe", s), nil)
	return f.Tools, e
}
func (c *Client) NeedsInspection(string) bool { return true }
func (c *Client) Invoke(ctx context.Context, s core.Session, scope core.ToolScope, call core.Call, progress func(json.RawMessage)) (core.Result, error) {
	in := input("invoke", s)
	in.Scope = scope
	in.Scope.Owner, in.Scope.Epoch = c.Owner, c.Epoch
	in.Call = call
	f, e := c.Do(ctx, in, progress)
	if e != nil {
		var failure *core.Error
		if errors.As(e, &failure) {
			return core.Result{}, e
		}
		return core.ToolError("tool service disconnected; execution outcome is unknown", true), nil
	}
	if f.Result == nil {
		return core.Result{}, errors.New("tools omitted result")
	}
	return *f.Result, nil
}
func (c *Client) CatalogContext(ctx context.Context, tenant string) ([]core.Tool, error) {
	f, e := c.Do(ctx, Request{Operation: "catalog", Tenant: tenant}, nil)
	return f.Tools, e
}
func (c *Client) CloseSession(tenant, id string) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_, _ = c.Do(ctx, Request{Operation: "close", Tenant: tenant, Session: core.Session{ID: id}}, nil)
}
