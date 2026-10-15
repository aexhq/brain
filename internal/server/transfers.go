package server

import (
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"strconv"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Server) nodes(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	values, err := s.Control.Nodes(r.Context(), r.URL.Query().Get("after"), limit)
	if err != nil {
		return err
	}
	return writeJSON(w, values)
}

func (s *Server) transferCreate(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.TransferCreate](s, w, r, "TransferCreate")
	if err != nil {
		return err
	}
	value, err := s.Control.BeginTransfer(r.Context(), r.PathValue("session"), request)
	if err != nil {
		return err
	}
	return writeJSON(w, value.View)
}

func (s *Server) transferRead(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	value, err := s.Control.Transfer(r.Context(), r.PathValue("session"), r.PathValue("transfer"))
	if err != nil {
		return err
	}
	return writeJSON(w, value.View)
}

func (s *Server) transferCancel(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	value, err := s.Control.CancelTransfer(r.Context(), r.PathValue("session"), r.PathValue("transfer"))
	if err != nil {
		return err
	}
	return writeJSON(w, value.View)
}

func (s *Server) coordinateTransfers(ctx context.Context) {
	defer s.wg.Done()
	ticker := time.NewTicker(250 * time.Millisecond)
	defer ticker.Stop()
	cursor := ""
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			values, err := s.Control.PendingTransfers(ctx, s.Config.Node, cursor, 4)
			if err != nil {
				continue
			}
			if len(values) == 0 {
				cursor = ""
			}
			for _, value := range values {
				cursor = value.View.ID
				if _, busy := s.busy.LoadOrStore(value.View.SessionID, true); busy {
					continue
				}
				step, cancel := context.WithTimeout(ctx, 30*time.Second)
				err := s.transferStep(step, value.View.SessionID, value.View.ID)
				cancel()
				s.busy.Delete(value.View.SessionID)
				if err != nil && ctx.Err() == nil {
					failure := kernel.Response(err)
					if failure.Code != wire.ErrorCodeConflict && failure.Code != wire.ErrorCodeOverloaded && failure.Code != wire.ErrorCodeUncertain {
						_ = s.Control.FailTransfer(ctx, value.View.SessionID, value.View.ID, s.Config.Node, failure)
					}
					slog.Warn("ownership_transfer_delayed", "transfer", value.View.ID, "code", failure.Code)
				}
			}
		}
	}
}

func (s *Server) bindResults(ctx context.Context, session control.Session) error {
	operations, err := s.Control.PendingResults(ctx, session.View.ID, 128)
	if err != nil {
		return err
	}
	for _, operation := range operations {
		usage, err := kernel.ReceiptUsage(operation.View.Usage, operation.ReceiptCounters)
		if err != nil {
			return err
		}
		receipt := wire.Receipt{RequestID: "evidence:" + operation.View.ID, Outcome: wire.Outcome(operation.View.Status),
			Result: operation.View.Result, Usage: usage, Error: operation.View.Error}
		kind := "operation.ended"
		if receipt.Outcome == wire.OutcomeFailed {
			kind = "operation.failed"
		}
		if receipt.Outcome == wire.OutcomeUnknown {
			kind = "operation.uncertain"
		}
		record, err := s.append(ctx, session, kind, receipt, operation.View.RunID, operation.View.ID, receipt.Outcome != wire.OutcomeUnknown)
		if err != nil {
			return err
		}
		sequence, _ := kernel.Sequence(record.Sequence)
		if err = s.Control.BindResult(ctx, session.View.ID, session.Owner, operation.View.ID, sequence, kernel.Digest(kernel.JSON(record))); err != nil {
			return err
		}
	}
	return nil
}

func (s *Server) transferStep(ctx context.Context, sessionID, id string) error {
	value, err := s.Control.Transfer(ctx, sessionID, id)
	if err != nil || (value.View.Phase != wire.TransferPhaseDraining && value.View.Phase != wire.TransferPhaseVerifying) {
		return err
	}
	session, err := s.Control.Session(ctx, "", sessionID)
	if err != nil {
		return err
	}
	if session.View.Lifecycle != wire.LifecycleOpen {
		return s.Control.FailTransfer(ctx, sessionID, id, s.Config.Node, kernel.Response(kernel.Fail(wire.ErrorCodeConflict, "session closed during planned transfer")))
	}
	if session.Owner.Node != value.View.SourceNode || session.View.OwnerEpoch != value.View.SourceEpoch {
		return kernel.Fail(wire.ErrorCodeConflict, "transfer source no longer owns the frozen epoch")
	}
	if !value.TargetLive {
		return s.Control.FailTransfer(ctx, sessionID, id, s.Config.Node, kernel.Response(kernel.Fail(wire.ErrorCodeUnavailable, "target incarnation expired before verified transfer")))
	}
	if value.View.SourceNode == s.Config.Node {
		if value.View.Phase == wire.TransferPhaseVerifying {
			if !value.Archived {
				return s.History.Flush(ctx, sessionID, session.Owner, session.Generation)
			}
			return nil
		}
		if err = s.bindResults(ctx, session); err != nil {
			return err
		}
		value, err = s.Control.Transfer(ctx, sessionID, id)
		if err != nil || !value.Drained {
			return err
		}
		record, err := s.append(ctx, session, "ownership.checkpoint", map[string]any{
			"format": "brain-ownership-checkpoint/1", "transfer_id": id,
			"source_epoch": value.View.SourceEpoch, "target_node": value.View.TargetNode,
		}, "", "", false)
		if err != nil {
			return err
		}
		return s.Control.SealTransfer(ctx, sessionID, session.Owner, id, record)
	}
	if value.View.TargetNode != s.Config.Node || value.View.Phase != wire.TransferPhaseVerifying || !value.Archived {
		return nil
	}
	if err = s.validateConfig(ctx, session.Tenant, session.View.Config); err != nil {
		return err
	}
	if !value.TailVerified {
		if value.View.Checkpoint == nil {
			return kernel.Fail(wire.ErrorCodeCorrupt, "transfer has no retained checkpoint reference")
		}
		through, err := kernel.Sequence(value.View.Through)
		if err != nil {
			return err
		}
		if _, err = s.History.Archived(ctx, sessionID, through, value.View.Checkpoint.Digest); err != nil {
			return err
		}
		if _, err = s.History.ArchivedContext(ctx, sessionID, through); err != nil {
			return err
		}
		return s.Control.VerifyTransferTail(ctx, sessionID, id, s.Config.Node, *value.View.Checkpoint)
	}
	runs, err := s.Control.TransferRuns(ctx, sessionID, id, value.Cursor, 4)
	if err != nil {
		return err
	}
	verified := []string{}
	for _, run := range runs {
		if err = s.Harness.Resume(ctx, session.View.Config, run.View.ID, run.HarnessProcess); err != nil {
			return err
		}
		if !json.Valid(run.Checkpoint) {
			return kernel.Fail(wire.ErrorCodeCorrupt, "retained harness checkpoint is not valid JSON")
		}
		if run.Decision != nil {
			if err = s.Validator.Wire("Decision", kernel.JSON(run.Decision)); err != nil {
				return err
			}
		}
		if _, err = s.History.ArchivedContext(ctx, sessionID, run.InputSequence); err != nil {
			return err
		}
		if err = s.verifyTransferWait(ctx, sessionID, run); err != nil {
			return err
		}
		verified = append(verified, run.View.ID)
	}
	_, err = s.Control.VerifyTransferRuns(ctx, sessionID, id, s.Config.Node, value.Cursor, verified)
	return err
}

func (s *Server) verifyTransferWait(ctx context.Context, session string, run control.Run) error {
	if run.View.Wait == nil {
		return nil
	}
	switch run.View.Wait.Kind {
	case "operation":
		operation, err := s.Control.Operation(ctx, session, run.View.Wait.ID)
		if err != nil {
			return err
		}
		for _, reference := range []struct {
			sequence int64
			digest   string
		}{{operation.IntentSequence, operation.IntentDigest}, {operation.ResultSequence, operation.ResultDigest}} {
			if reference.sequence != 0 {
				if _, err = s.History.Archived(ctx, session, reference.sequence, reference.digest); err != nil {
					return err
				}
			}
		}
	case "children":
		step, err := strconv.ParseUint(run.View.Wait.ID, 10, 32)
		if err != nil {
			return err
		}
		results, err := s.Control.Children(ctx, session, run.View.ID, step)
		if err != nil {
			return err
		}
		for _, child := range results.Children {
			if child.Completion != nil {
				sequence, err := kernel.Sequence(child.Completion.Sequence)
				if err != nil {
					return err
				}
				if _, err = s.History.Archived(ctx, child.Completion.SessionID, sequence, child.Completion.Digest); err != nil {
					return err
				}
			}
		}
	}
	return nil
}
