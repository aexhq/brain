package d1control

import (
	"context"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
)

func (s *Store) RunnableRuns(ctx context.Context, session string, limit int) (values []control.Run, err error) {
	err = s.call(ctx, "RunnableRuns", []any{session, limit}, &values)
	return
}
func (s *Store) PendingResults(ctx context.Context, session string, limit int) (values []control.Operation, err error) {
	err = s.call(ctx, "PendingResults", []any{session, limit}, &values)
	return
}
func (s *Store) ChildState(ctx context.Context, session string) (value control.ChildState, err error) {
	err = s.call(ctx, "ChildState", []any{session}, &value)
	return
}
func (s *Store) Interaction(ctx context.Context, session, id string) (value wire.Interaction, err error) {
	err = s.call(ctx, "Interaction", []any{session, id}, &value)
	return
}
func (s *Store) ActiveConnection(ctx context.Context, session, template string) (value wire.Connection, err error) {
	err = s.call(ctx, "ActiveConnection", []any{session, template}, &value)
	return
}
func (s *Store) File(ctx context.Context, session, name string) (value wire.FileInfo, err error) {
	err = s.call(ctx, "File", []any{session, name}, &value)
	return
}
