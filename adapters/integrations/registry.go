package integrations

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"time"

	"github.com/aexhq/brain/adapters/models"
	"github.com/aexhq/brain/adapters/network"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type Handler interface {
	Invoke(context.Context, string, wire.ConnectionTemplate, wire.Execution) (wire.Receipt, error)
	Validate(wire.ConnectionTemplate) error
}

type Registry struct {
	Models   *models.Registry
	Handlers map[string]Handler
	Entries  []wire.CatalogEntry
	Secrets  []string
}

func (r *Registry) Validate(ctx context.Context, tenant string, config wire.SessionConfig) error {
	if err := r.Models.Validate(config.Model); err != nil {
		return err
	}
	if _, err := r.Models.Resolve(ctx, tenant, r.Models.Credential(config.Model)); err != nil {
		return err
	}
	for _, connection := range config.Connections {
		if connection.Kind == wire.ConnectionKindApplication {
			continue
		}
		handler := r.Handlers[connection.Provider]
		if handler == nil {
			return kernel.Fail(wire.ErrorCodeUnsupported, "connection provider is not installed")
		}
		if err := handler.Validate(connection); err != nil {
			return err
		}
		for _, ref := range secretRefs(handler, connection) {
			if _, err := r.Models.Resolve(ctx, tenant, ref); err != nil {
				return err
			}
		}
		if admitted, ok := handler.(interface {
			ValidateAction(context.Context, string, wire.ConnectionTemplate, wire.Action) error
		}); ok {
			for _, action := range config.Actions {
				if action.Connection == connection.Name {
					if err := admitted.ValidateAction(ctx, tenant, connection, action); err != nil {
						return err
					}
				}
			}
		}
	}
	return nil
}

func secretRefs(handler Handler, template wire.ConnectionTemplate) []string {
	out := []string{}
	for _, ref := range template.Secrets {
		out = append(out, ref)
	}
	if additional, ok := handler.(interface{ Secrets() []string }); ok {
		out = append(out, additional.Secrets()...)
	}
	return out
}

func (r *Registry) capability(provider string) string {
	identity := provider
	if scoped, ok := r.Handlers[provider].(interface{ Identity() string }); ok {
		identity = scoped.Identity()
	}
	return kernel.Capability("effect", provider, identity)
}

func (r *Registry) Capabilities() []string {
	out := append([]string{}, r.Secrets...)
	for name := range r.Models.Providers {
		out = append(out, r.Models.Capability(name))
	}
	for name := range r.Handlers {
		out = append(out, r.capability(name))
	}
	return out
}

func (r *Registry) Requirements(tenant string, config wire.SessionConfig) []string {
	out := []string{r.Models.Capability(config.Model.Provider), kernel.Capability("secret", tenant, r.Models.Credential(config.Model))}
	for _, connection := range config.Connections {
		if connection.Kind == wire.ConnectionKindApplication {
			continue
		}
		out = append(out, r.capability(connection.Provider))
		for _, ref := range secretRefs(r.Handlers[connection.Provider], connection) {
			out = append(out, kernel.Capability("secret", tenant, ref))
		}
	}
	return out
}
func (r *Registry) Model(ctx context.Context, tenant string, model wire.ModelSelection, request wire.ModelRequest, limit int64) (wire.Receipt, error) {
	return r.Models.Call(ctx, tenant, model, request, limit)
}
func (r *Registry) Action(ctx context.Context, tenant string, template wire.ConnectionTemplate, execution wire.Execution) (wire.Receipt, error) {
	handler := r.Handlers[template.Provider]
	if handler == nil {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeUnsupported, "connection provider is not installed")
	}
	return handler.Invoke(ctx, tenant, template, execution)
}
func (r *Registry) Catalog() []wire.CatalogEntry {
	entries := []wire.CatalogEntry{}
	if json.Unmarshal(wire.Catalog, &entries) != nil {
		panic("generated integration catalog is invalid")
	}
	out := []wire.CatalogEntry{}
	for _, entry := range entries {
		if r.Handlers[entry.Name] != nil || entry.Name == "application" {
			out = append(out, entry)
		}
	}
	return append(out, r.Entries...)
}

type HTTP struct {
	Resolve     models.Resolver
	Client      *http.Client
	Development bool
}
type endpoint struct {
	URL string `json:"url"`
}

func (h *HTTP) Validate(template wire.ConnectionTemplate) error {
	var config endpoint
	if err := json.Unmarshal(template.Configuration, &config); err != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "HTTP connection requires a URL")
	}
	u, err := url.Parse(config.URL)
	if err != nil || u.User != nil || u.Hostname() == "" || (u.Scheme != "https" && !(h.Development && u.Scheme == "http" && (u.Hostname() == "127.0.0.1" || u.Hostname() == "localhost"))) {
		return kernel.Fail(wire.ErrorCodeInvalid, "application endpoint must use authenticated HTTPS")
	}
	if template.Secrets["signing"] == "" {
		return kernel.Fail(wire.ErrorCodeInvalid, "application HTTP connection requires a scoped signing secret")
	}
	if !h.Development {
		if _, err = network.ValidateURL(config.URL); err != nil {
			return err
		}
	}
	return nil
}

func (h *HTTP) Invoke(ctx context.Context, tenant string, template wire.ConnectionTemplate, execution wire.Execution) (wire.Receipt, error) {
	return h.Send(ctx, tenant, template, kernel.JSON(execution), execution.Operation.ID, execution.MaxOutputBytes)
}

func (h *HTTP) Send(ctx context.Context, tenant string, template wire.ConnectionTemplate, body []byte, operation string, maxOutput uint64) (wire.Receipt, error) {
	if err := h.Validate(template); err != nil {
		return wire.Receipt{}, err
	}
	var config endpoint
	_ = json.Unmarshal(template.Configuration, &config)
	key, err := h.Resolve(ctx, tenant, template.Secrets["signing"])
	if err != nil {
		return wire.Receipt{}, err
	}
	mac := hmac.New(sha256.New, []byte(key))
	_, _ = mac.Write(body)
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, config.URL, bytes.NewReader(body))
	if err != nil {
		return wire.Receipt{}, err
	}
	request.GetBody = nil
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Brain-Operation", operation)
	request.Header.Set("X-Brain-Signature", "v1="+hex.EncodeToString(mac.Sum(nil)))
	client := h.Client
	if client == nil {
		client = network.Client(5 * time.Minute)
		if h.Development {
			client = &http.Client{Timeout: 5 * time.Minute, Transport: &http.Transport{DisableKeepAlives: true}}
		}
		client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	}
	response, err := client.Do(request)
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("application request may have been accepted; no automatic retry")}, nil
	}
	defer response.Body.Close()
	result, err := io.ReadAll(io.LimitReader(response.Body, int64(maxOutput)+4097))
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("application result transport failed")}, nil
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("application returned no authoritative completion receipt")}, nil
	}
	if len(result) > int(maxOutput)+4096 {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("application receipt exceeded its admitted bound")}, nil
	}
	var receipt wire.Receipt
	if err = json.Unmarshal(result, &receipt); err != nil || (receipt.Outcome != wire.OutcomeCompleted && receipt.Outcome != wire.OutcomeFailed && receipt.Outcome != wire.OutcomeUnknown) {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("application returned an invalid completion receipt")}, nil
	}
	return receipt, nil
}
