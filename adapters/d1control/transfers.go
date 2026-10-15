package d1control

import (
	"context"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
)

func (s *Store) Nodes(ctx context.Context, after string, limit int) (out []wire.NodeInfo, err error) {
	err = s.call(ctx, "Nodes", []any{after, limit}, &out)
	return
}
func (s *Store) BeginTransfer(ctx context.Context, session string, request wire.TransferCreate) (out control.Transfer, err error) {
	err = s.call(ctx, "BeginTransfer", []any{session, request}, &out)
	return
}
func (s *Store) Transfer(ctx context.Context, session, id string) (out control.Transfer, err error) {
	err = s.call(ctx, "Transfer", []any{session, id}, &out)
	return
}
func (s *Store) PendingTransfers(ctx context.Context, node, after string, limit int) (out []control.Transfer, err error) {
	err = s.call(ctx, "PendingTransfers", []any{node, after, limit}, &out)
	return
}
func (s *Store) SealTransfer(ctx context.Context, session string, owner control.Owner, id string, record wire.Record) error {
	return s.call(ctx, "SealTransfer", []any{session, owner, id, record}, nil)
}
func (s *Store) TransferRuns(ctx context.Context, session, id, after string, limit int) (out []control.Run, err error) {
	err = s.call(ctx, "TransferRuns", []any{session, id, after, limit}, &out)
	return
}
func (s *Store) VerifyTransferTail(ctx context.Context, session, id, node string, reference wire.RecordReference) error {
	return s.call(ctx, "VerifyTransferTail", []any{session, id, node, reference}, nil)
}
func (s *Store) VerifyTransferRuns(ctx context.Context, session, id, node, after string, runs []string) (out control.Transfer, err error) {
	if runs == nil {
		runs = []string{}
	}
	err = s.call(ctx, "VerifyTransferRuns", []any{session, id, node, after, runs}, &out)
	return
}
func (s *Store) CancelTransfer(ctx context.Context, session, id string) (out control.Transfer, err error) {
	err = s.call(ctx, "CancelTransfer", []any{session, id}, &out)
	return
}
func (s *Store) FailTransfer(ctx context.Context, session, id, node string, failure wire.ErrorResponse) error {
	return s.call(ctx, "FailTransfer", []any{session, id, node, failure}, nil)
}
