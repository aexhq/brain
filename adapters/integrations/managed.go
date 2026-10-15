package integrations

import (
	"context"
	"encoding/json"
	"slices"
	"strings"

	"github.com/aexhq/brain/adapters/models"
	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type Managed struct {
	Provider    string
	URL         string
	Signing     string
	PublicURL   string
	Control     control.Store
	Resolve     models.Resolver
	Development bool
}

func (h *Managed) Identity() string  { return kernel.ID(h.Provider, h.URL, h.Signing) }
func (h *Managed) Secrets() []string { return []string{h.Signing} }

func (h *Managed) transport() (*HTTP, wire.ConnectionTemplate) {
	return &HTTP{Resolve: h.Resolve, Development: h.Development}, wire.ConnectionTemplate{
		Configuration: kernel.JSON(endpoint{URL: h.URL}), Secrets: map[string]string{"signing": h.Signing},
	}
}

func (h *Managed) Validate(template wire.ConnectionTemplate) error {
	if template.Ownership != wire.OwnershipManaged || template.Provider != h.Provider || h.PublicURL == "" {
		return kernel.Fail(wire.ErrorCodeInvalid, "managed compute requires managed ownership and a registered worker and public callback origin")
	}
	if h.Provider == "local_compute" && (!h.Development || template.Kind != wire.ConnectionKindLocal) {
		return kernel.Fail(wire.ErrorCodeForbidden, "local compute is only an explicitly trusted development runner")
	}
	for _, capability := range template.Capabilities {
		if capability != "build" && capability != "invoke" {
			return kernel.Fail(wire.ErrorCodeUnsupported, "managed compute supports build and invoke capabilities")
		}
	}
	http, target := h.transport()
	return http.Validate(target)
}

func (h *Managed) ValidateAction(ctx context.Context, tenant string, template wire.ConnectionTemplate, action wire.Action) error {
	if action.Name == "build_package" {
		if !slices.Contains(template.Capabilities, "build") || action.Package != nil {
			return kernel.Fail(wire.ErrorCodeForbidden, "package build needs an admitted build capability")
		}
		return nil
	}
	if !slices.Contains(template.Capabilities, "invoke") || action.Package == nil {
		return kernel.Fail(wire.ErrorCodeForbidden, "managed actions require invoke capability and an immutable package identifier")
	}
	pkg, err := h.Control.Package(ctx, tenant, *action.Package)
	if err != nil {
		return err
	}
	if pkg.Status != "ready" || pkg.Image == nil {
		return kernel.Fail(wire.ErrorCodeUnavailable, "managed action package has no verified build")
	}
	if h.Provider == "local_compute" && len(pkg.Manifest.Network) != 0 {
		return kernel.Fail(wire.ErrorCodeUnsupported, "trusted local compute does not provide admitted application network access")
	}
	for _, name := range pkg.Manifest.Secrets {
		if template.Secrets[name] == "" {
			return kernel.Fail(wire.ErrorCodeForbidden, "managed package secret has no admitted scoped reference")
		}
	}
	for _, declared := range pkg.Manifest.Actions {
		if declared.Name == action.Name && kernel.Digest(declared.InputSchema) == kernel.Digest(action.InputSchema) &&
			kernel.Digest(declared.OutputSchema) == kernel.Digest(action.OutputSchema) &&
			action.MaxOutputBytes <= declared.MaxOutputBytes && action.TimeoutMs <= declared.TimeoutMs {
			return nil
		}
	}
	return kernel.Fail(wire.ErrorCodeForbidden, "managed action does not match its package contract")
}

func (h *Managed) Invoke(ctx context.Context, tenant string, template wire.ConnectionTemplate, execution wire.Execution) (wire.Receipt, error) {
	reject := func(message string) (wire.Receipt, error) {
		return wire.Receipt{Outcome: wire.OutcomeFailed, Error: &message}, nil
	}
	if err := h.Validate(template); err != nil {
		return reject("managed placement validation failed before dispatch")
	}
	session, err := h.Control.Session(ctx, tenant, execution.Operation.SessionID)
	if err != nil {
		return reject("managed session is unavailable")
	}
	action, err := kernel.Action(session.View.Config, execution.Operation.Action)
	if err != nil {
		return reject("managed action is no longer admitted")
	}
	if err = h.ValidateAction(ctx, tenant, template, action); err != nil {
		return reject("managed package or capability validation failed before worker dispatch")
	}
	id := kernel.Value(action.Package)
	if action.Name == "build_package" {
		var arguments struct {
			PackageID string `json:"package_id"`
		}
		if json.Unmarshal(execution.Arguments, &arguments) != nil {
			return reject("invalid package build arguments")
		}
		id = arguments.PackageID
	}
	pkg, err := h.Control.Package(ctx, tenant, id)
	if err != nil {
		return reject("managed package does not belong to this tenant")
	}
	invocation := wire.ManagedInvocation{Execution: execution, Package: pkg, Secrets: map[string]string{},
		CallbackUrl: strings.TrimRight(h.PublicURL, "/") + "/v2/sessions/" + execution.Operation.SessionID + "/operations/" + execution.Operation.ID + "/receipt"}
	if action.Name != "build_package" {
		for _, name := range pkg.Manifest.Secrets {
			value, err := h.Resolve(ctx, tenant, template.Secrets[name])
			if err != nil {
				return reject("managed secret resolution failed before worker dispatch")
			}
			invocation.Secrets[name] = value
		}
	}
	http, target := h.transport()
	return http.Send(ctx, tenant, target, kernel.JSON(invocation), execution.Operation.ID, execution.MaxOutputBytes)
}
