package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
	"golang.org/x/net/websocket"
	"io"
	"net"
	"net/http"
	"strings"
	"time"
)

type Client struct {
	URL, Token string
	HTTP       *http.Client
}

func (c *Client) request(ctx context.Context, method, path string, body io.Reader) (*http.Response, error) {
	r, e := http.NewRequestWithContext(ctx, method, strings.TrimRight(c.URL, "/")+path, body)
	if e != nil {
		return nil, e
	}
	r.Header.Set("Authorization", "Bearer "+c.Token)
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(r.Header))
	h := c.HTTP
	if h == nil {
		h = &http.Client{Timeout: 60 * time.Second}
	}
	response, e := h.Do(r)
	if e != nil {
		return nil, e
	}
	if response.StatusCode >= 300 {
		defer response.Body.Close()
		var v Failure
		_ = json.NewDecoder(io.LimitReader(response.Body, 4096)).Decode(&v)
		if v.Error == "" {
			v.Error = "sandbox request failed"
		}
		return nil, errors.New(v.Error)
	}
	return response, nil
}
func (c *Client) Create(ctx context.Context, input Create) (Environment, error) {
	var out Environment
	b, e := json.Marshal(input)
	if e != nil {
		return out, e
	}
	r, e := c.request(ctx, "POST", "/environments", bytes.NewReader(b))
	if e != nil {
		return out, e
	}
	defer r.Body.Close()
	e = json.NewDecoder(io.LimitReader(r.Body, 4096)).Decode(&out)
	return out, e
}
func (c *Client) Remove(ctx context.Context, id string) error {
	r, e := c.request(ctx, "DELETE", "/environments/"+id, nil)
	if e == nil {
		r.Body.Close()
	}
	return e
}
func (c *Client) Import(ctx context.Context, id string, tar io.Reader) error {
	r, e := c.request(ctx, "PUT", "/environments/"+id+"/files", tar)
	if e == nil {
		r.Body.Close()
	}
	return e
}
func (c *Client) Export(ctx context.Context, id string, w io.Writer, limit int64) error {
	r, e := c.request(ctx, "GET", "/environments/"+id+"/files", nil)
	if e != nil {
		return e
	}
	defer r.Body.Close()
	n, e := io.Copy(w, io.LimitReader(r.Body, limit+1))
	if e == nil && n > limit {
		e = errors.New("sandbox export exceeds limit")
	}
	return e
}
func (c *Client) Process(ctx context.Context, id string, input Process) (io.ReadWriteCloser, error) {
	u := strings.TrimRight(c.URL, "/")
	wsURL := strings.Replace(strings.Replace(u, "https://", "wss://", 1), "http://", "ws://", 1) + "/environments/" + id + "/process"
	cfg, e := websocket.NewConfig(wsURL, u)
	if e != nil {
		return nil, e
	}
	cfg.Header.Set("Authorization", "Bearer "+c.Token)
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(cfg.Header))
	cfg.Dialer = &net.Dialer{Timeout: 10 * time.Second}
	ws, e := websocket.DialConfig(cfg)
	if e != nil {
		return nil, e
	}
	ws.MaxPayloadBytes = 4 << 20
	if e = websocket.JSON.Send(ws, input); e != nil {
		ws.Close()
		return nil, e
	}
	var response Failure
	if e = websocket.JSON.Receive(ws, &response); e != nil {
		ws.Close()
		return nil, e
	}
	if response.Error != "" {
		ws.Close()
		return nil, errors.New(response.Error)
	}
	stop := context.AfterFunc(ctx, func() { _ = ws.Close() })
	return &stream{ReadWriteCloser: ws, stop: stop}, nil
}

type stream struct {
	io.ReadWriteCloser
	stop func() bool
}

func (s *stream) Close() error { s.stop(); return s.ReadWriteCloser.Close() }
