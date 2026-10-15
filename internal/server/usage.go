package server

import (
	"net/http"
	"strings"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Server) receiptOperation(r *http.Request) (control.Operation, error) {
	fields := strings.Fields(r.Header.Get("Authorization"))
	if len(r.Header.Values("Authorization")) != 1 || len(fields) != 2 || !strings.EqualFold(fields[0], "Bearer") ||
		len(fields[1]) > 512 || !strings.HasPrefix(fields[1], "r_"+r.PathValue("operation")+".") {
		return control.Operation{}, kernel.Fail(wire.ErrorCodeUnauthorized, "receipt authority is invalid")
	}
	operation, err := s.Control.Operation(r.Context(), r.PathValue("session"), r.PathValue("operation"))
	if err != nil {
		if kernel.Response(err).Code == wire.ErrorCodeNotFound {
			return control.Operation{}, kernel.Fail(wire.ErrorCodeUnauthorized, "receipt authority is invalid")
		}
		return control.Operation{}, err
	}
	if !hmacEqual(fields[1], s.receiptToken(operation)) {
		return control.Operation{}, kernel.Fail(wire.ErrorCodeUnauthorized, "receipt authority is invalid")
	}
	return operation, nil
}

func (s *Server) reportUsage(w http.ResponseWriter, r *http.Request) {
	err := func() error {
		var reporter control.UsageReporter
		fields := strings.Fields(r.Header.Get("Authorization"))
		if len(fields) == 2 && strings.HasPrefix(fields[1], "r_") {
			operation, err := s.receiptOperation(r)
			if err != nil {
				return err
			}
			reporter = control.UsageReporter{Kind: wire.UsageSourceReceipt, Subject: operation.Claimant}
		} else {
			principal, err := s.principal(r)
			if err != nil {
				return err
			}
			if !principal.Operator {
				return kernel.Fail(wire.ErrorCodeForbidden, "model usage requires an operator attestation or the original receipt capability")
			}
			reporter = control.UsageReporter{Kind: wire.UsageSourceOperator, Tenant: principal.Tenant, Subject: principal.Subject}
		}
		request, err := readBody[wire.UsageSupplement](s, w, r, "UsageSupplement")
		if err != nil {
			return err
		}
		value, err := s.Control.ReportUsage(r.Context(), r.PathValue("session"), r.PathValue("operation"), request, reporter)
		if err != nil {
			return err
		}
		return writeJSON(w, value.View)
	}()
	if err != nil {
		writeError(w, err)
	}
}

func (s *Server) usageEvidence(w http.ResponseWriter, r *http.Request, _ kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	session, operation := r.PathValue("session"), r.PathValue("operation")
	if _, err := s.Control.Operation(r.Context(), session, operation); err != nil {
		return err
	}
	values, err := s.Control.UsageEvidence(r.Context(), session, operation, limit, r.URL.Query().Get("after"))
	if err != nil {
		return err
	}
	return writeJSON(w, values)
}
