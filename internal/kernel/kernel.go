// Package kernel contains deterministic admission and transition rules. It has
// no transport, database, provider SDK, process or clock dependencies.
package kernel

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"slices"
	"strconv"

	"github.com/aexhq/brain/internal/generated/wire"
)

type Fault struct{ Response wire.ErrorResponse }

func (e *Fault) Error() string { return e.Response.Message }

func Fail(code wire.ErrorCode, message string) error {
	return &Fault{wire.ErrorResponse{Code: code, Message: message, Retryable: code == wire.ErrorCodeOverloaded}}
}

func Response(err error) wire.ErrorResponse {
	if err == nil {
		return wire.ErrorResponse{}
	}
	var fault *Fault
	if errors.As(err, &fault) {
		return fault.Response
	}
	return wire.ErrorResponse{Code: wire.ErrorCodeInternal, Message: "internal operation failed; inspect the request trace"}
}

func Ptr[T any](value T) *T { return &value }

func Value[T any](value *T) (zero T) {
	if value != nil {
		return *value
	}
	return
}

func JSON(value any) json.RawMessage {
	b, err := json.Marshal(value)
	if err != nil {
		panic(fmt.Sprintf("internal non-JSON value: %v", err))
	}
	return b
}

func Digest(value []byte) string {
	sum := sha256.Sum256(value)
	return hex.EncodeToString(sum[:])
}

func ID(parts ...string) string { return Digest(JSON(parts))[:40] }

func Capability(kind string, parts ...string) string {
	return kind + ":" + ID(parts...)
}

func HarnessCapability(harness wire.HarnessSelection, model string) string {
	return Capability("harness", harness.Name, harness.Version, model)
}

func Sequence(value string) (int64, error) {
	n, err := strconv.ParseInt(value, 10, 64)
	if err != nil || n < 0 || strconv.FormatInt(n, 10) != value {
		return 0, Fail(wire.ErrorCodeInvalid, "sequence must be a canonical non-negative decimal string")
	}
	return n, nil
}

var identifier = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)

func Identifier(value string) bool { return identifier.MatchString(value) }

type Principal struct {
	Tenant      string
	Subject     string
	Session     string
	GrantID     string
	Permissions []string
	Operator    bool
	Issuer      string
	Expires     int64
}

func (p Principal) Allows(session, permission string) bool {
	return p.Operator || ((p.Session == "" || p.Session == session) &&
		(slices.Contains(p.Permissions, "*") || slices.Contains(p.Permissions, permission)))
}

func Subset(child, parent []string) bool {
	for _, item := range child {
		if !slices.Contains(parent, item) {
			return false
		}

	}
	return true
}

func PermissionSubset(child, parent []string) bool {
	return slices.Contains(parent, "*") || Subset(child, parent)
}

func ValidateConfig(config wire.SessionConfig) error {
	l := config.Limits
	if l.Calls == 0 || l.Calls > 1000000 || l.Concurrent == 0 || l.Concurrent > 1024 ||
		l.InputBytes == 0 || l.InputBytes > 262144 || l.OutputBytes == 0 || l.OutputBytes > 16777216 ||
		l.RetainedBytes < l.OutputBytes*2 || l.RetainedBytes > 1<<50 ||
		l.PendingInputs == 0 || l.PendingInputs > 4096 {
		return Fail(wire.ErrorCodeInvalid, "invalid call, concurrency, input, output, retained or pending-input limits")
	}
	if (l.Tokens != nil && *l.Tokens > 1<<53-1) || (l.DeadlineMs != nil && *l.DeadlineMs > 1<<53-1) {
		return Fail(wire.ErrorCodeInvalid, "token and time limits must fit exact language-neutral integers")
	}
	if len(config.Actions) > 128 || len(config.Connections) > 32 || len(config.Events) > 128 ||
		len(config.Instructions) > 65536 || len(config.Skills) > 32 || len(config.Grants) > 128 ||
		len(config.HarnessTransitions) > 32 || len(JSON(config)) > 262144 {
		return Fail(wire.ErrorCodeInvalid, "session catalogue exceeds its bounded admission size")
	}
	for _, grant := range config.Grants {
		if grant != "*" && !Identifier(grant) {
			return Fail(wire.ErrorCodeInvalid, "session grant is not a bounded identifier")
		}
	}
	connections := map[string]bool{}
	for _, connection := range config.Connections {
		if !Identifier(connection.Name) || connections[connection.Name] {
			return Fail(wire.ErrorCodeInvalid, "connection template names must be unique identifiers")
		}
		connections[connection.Name] = true
	}
	actions := map[string]bool{}
	for _, action := range config.Actions {
		if !Identifier(action.Name) || actions[action.Name] || !connections[action.Connection] ||
			action.MaxOutputBytes == 0 || action.MaxOutputBytes > l.OutputBytes ||
			action.TimeoutMs == 0 || action.TimeoutMs > 86400000 {
			return Fail(wire.ErrorCodeInvalid, "invalid action name, placement, output limit or timeout")
		}
		actions[action.Name] = true
	}
	routes := map[string]bool{}
	for _, route := range config.Events {
		key := route.Name + ":" + route.Version
		if !Identifier(route.Name) || !Identifier(route.Version) || routes[key] ||
			len(route.Sources) > 64 || (route.Action != nil && !actions[*route.Action]) {
			return Fail(wire.ErrorCodeInvalid, "invalid or duplicate event route")
		}
		routes[key] = true
		for _, source := range route.Sources {
			if !Identifier(source) {
				return Fail(wire.ErrorCodeInvalid, "event source must be a bounded identifier")
			}
		}
	}
	return nil
}

func Action(config wire.SessionConfig, name string) (wire.Action, error) {
	for _, action := range config.Actions {
		if action.Name == name {
			return action, nil
		}
	}
	return wire.Action{}, Fail(wire.ErrorCodeForbidden, "action is outside the immutable session catalogue")
}

func Route(config wire.SessionConfig, name, version, source string) (wire.EventRoute, error) {
	for _, route := range config.Events {
		if route.Name == name && route.Version == version {
			if len(route.Sources) > 0 && !slices.Contains(route.Sources, source) {
				return wire.EventRoute{}, Fail(wire.ErrorCodeForbidden, "event source is not admitted")
			}
			return route, nil
		}
	}
	return wire.EventRoute{}, Fail(wire.ErrorCodeInvalid, "event type and version are not admitted")
}

func Terminal(status wire.RunStatus) bool {
	return status == wire.RunStatusCancelled || status == wire.RunStatusCompleted ||
		status == wire.RunStatusFailed || status == wire.RunStatusBlocked
}

func OutcomeStatus(outcome wire.Outcome) wire.OperationStatus {
	switch outcome {
	case wire.OutcomeCompleted:
		return wire.OperationStatusCompleted
	case wire.OutcomeFailed:
		return wire.OperationStatusFailed
	default:
		return wire.OperationStatusUnknown
	}
}
