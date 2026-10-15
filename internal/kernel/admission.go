package kernel

import (
	"strings"

	"github.com/aexhq/brain/internal/generated/wire"
)

func ValidateParticipant(request wire.ParticipantGrant) error {
	subject := strings.ToLower(request.Subject)
	if !Identifier(request.RequestID) || !Identifier(request.Subject) ||
		subject == "kernel" || subject == "operator" || subject == "creator" ||
		subject == "timer" || subject == "local" || strings.HasPrefix(subject, "harness:") {
		return Fail(wire.ErrorCodeInvalid, "participant subject must be a bounded non-system identifier")
	}
	if len(request.Permissions) > 64 {
		return Fail(wire.ErrorCodeInvalid, "participant permissions exceed their bound")
	}
	for _, permission := range request.Permissions {
		if permission != "*" && !Identifier(permission) {
			return Fail(wire.ErrorCodeInvalid, "participant permission is not a bounded identifier")
		}
	}
	return nil
}

func ValidateInteraction(request wire.InteractionCreate) error {
	if !Identifier(request.RequestID) || len(JSON(request)) > 65536 ||
		len(request.Prompt) > 32768 || len(request.Subject) > 256 ||
		len(request.SubjectDigest) > 128 || len(request.RunID) > 128 {
		return Fail(wire.ErrorCodeInvalid, "interaction exceeds its bounded request fields")
	}
	return nil
}

func ValidateAnswer(answer wire.InteractionAnswer) error {
	if !Identifier(answer.RequestID) || len(JSON(answer)) > 65536 {
		return Fail(wire.ErrorCodeInvalid, "interaction answer exceeds its bounded fields")
	}
	return nil
}

func ValidateAttachment(request wire.ConnectionAttach) error {
	if !Identifier(request.RequestID) || !Identifier(request.Template) ||
		len(Value(request.ResourceID)) > 1024 {
		return Fail(wire.ErrorCodeInvalid, "connection attachment exceeds its bounded fields")
	}
	return nil
}
