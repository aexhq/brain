package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
	"io"
	"net/http"
	"strings"
	"time"
)

type Client struct {
	URL, Token string
	HTTP       *http.Client
}

func (c *Client) Do(ctx context.Context, input Request) (Response, error) {
	var out Response
	b, err := json.Marshal(input)
	if err != nil {
		return out, err
	}
	r, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.URL, "/")+"/rpc", bytes.NewReader(b))
	if err != nil {
		return out, err
	}
	r.Header.Set("Content-Type", "application/json")
	r.Header.Set("Authorization", "Bearer "+c.Token)
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(r.Header))
	h := c.HTTP
	if h == nil {
		h = &http.Client{Timeout: 60 * time.Second}
	}
	response, err := h.Do(r)
	if err != nil {
		return out, err
	}
	defer response.Body.Close()
	d := json.NewDecoder(io.LimitReader(response.Body, 4<<20))
	if err = d.Decode(&out); err != nil {
		return out, err
	}
	if out.Error != nil {
		return out, out.Error
	}
	if response.StatusCode != http.StatusOK {
		return out, errors.New("storage request failed")
	}
	return out, nil
}

func (c *Client) PutObject(ctx context.Context, identity Identity, object Object) (string, error) {
	r, err := c.Do(ctx, Request{Operation: "object.put", Identity: identity, Object: &object})
	return r.Reference, err
}
func (c *Client) GetObject(ctx context.Context, identity Identity, reference string) (Object, error) {
	r, err := c.Do(ctx, Request{Operation: "object.get", Identity: identity, Reference: reference})
	if err != nil {
		return Object{}, err
	}
	if r.Object == nil {
		return Object{}, errors.New("storage omitted object")
	}
	return *r.Object, nil
}
