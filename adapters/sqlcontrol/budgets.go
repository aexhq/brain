package sqlcontrol

import (
	"context"
	"database/sql"
	"strconv"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type budgetPool struct {
	id                                                   string
	calls, active, reserved, retained, tokens, held      int64
	incomplete, maxCalls, maxActive, maxBytes, maxTokens int64
	paused, generation, admitted                         int64
}

func (s *Store) budgetPools(ctx context.Context, q queryer, session string, lock bool) ([]budgetPool, error) {
	query := `SELECT b.id,b.calls,b.active,b.reserved,b.retained,b.tokens,b.reserved_tokens,
b.incomplete_usages,b.max_calls,b.max_active,b.max_bytes,b.max_tokens,b.paused,b.cancel_generation,m.generation
FROM budgets b JOIN budget_members m ON m.budget_id=b.id WHERE m.session_id=? ORDER BY b.id`
	if lock && s.postgres {
		query += " FOR UPDATE OF b"
	}
	rows, err := q.QueryContext(ctx, s.q(query), session)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []budgetPool{}
	for rows.Next() {
		var value budgetPool
		if err = rows.Scan(&value.id, &value.calls, &value.active, &value.reserved, &value.retained,
			&value.tokens, &value.held, &value.incomplete, &value.maxCalls, &value.maxActive, &value.maxBytes, &value.maxTokens,
			&value.paused, &value.generation, &value.admitted); err != nil {
			return nil, err
		}
		out = append(out, value)
		if len(out) > kernel.MaxDelegationDepth+1 {
			return nil, kernel.Fail(wire.ErrorCodeCorrupt, "budget ancestry exceeds its admitted depth")
		}
	}
	if len(out) == 0 {
		return nil, kernel.Fail(wire.ErrorCodeCorrupt, "session has no admitted budget pool")
	}
	return out, rows.Err()
}

func (s *Store) reserveBudgets(ctx context.Context, tx *sql.Tx, session string, bytes, tokens int64) error {
	pools, err := s.budgetPools(ctx, tx, session, true)
	if err != nil {
		return err
	}
	for _, pool := range pools {
		if pool.calls >= pool.maxCalls {
			return kernel.Fail(wire.ErrorCodeForbidden, "session or ancestor call limit reached")
		}
		if pool.active >= pool.maxActive || pool.retained+pool.reserved+bytes > pool.maxBytes {
			return kernel.Fail(wire.ErrorCodeOverloaded, "session or ancestor concurrency or output reserve is exhausted")
		}
		if pool.maxTokens >= 0 && tokens > 0 && pool.tokens+pool.held+tokens > pool.maxTokens {
			if pool.held == 0 {
				return kernel.Fail(wire.ErrorCodeForbidden, "model request does not fit the remaining token budget")
			}
			return kernel.Fail(wire.ErrorCodeOverloaded, "token budget is held by in-flight or incomplete usage")
		}
	}
	for _, pool := range pools {
		if err = requireOne(tx.ExecContext(ctx, s.q(`UPDATE budgets SET calls=calls+1,active=active+1,reserved=reserved+?,reserved_tokens=reserved_tokens+?
WHERE id=? AND calls<max_calls AND active<max_active AND retained+reserved+?<=max_bytes
AND (?=0 OR max_tokens<0 OR tokens+reserved_tokens+?<=max_tokens)`), bytes, tokens, pool.id, bytes, tokens, tokens)); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) settleBudgets(ctx context.Context, tx *sql.Tx, operation control.Operation, receipt wire.Receipt, bodyBytes int, notSent, bound bool) error {
	pools, err := s.budgetPools(ctx, tx, operation.View.SessionID, true)
	if err != nil {
		return err
	}
	reported, complete := int64(0), true
	if !notSent && operation.View.Kind == wire.EffectKindModel {
		reported, complete = kernel.ReportedTokens(receipt.Usage)
	}
	released, incomplete := operation.TokenReserve, int64(0)
	if !complete {
		released, incomplete = 0, 1
	}
	var reserve int64
	if bound {
		reserve = kernel.OutputReservation(operation.OutputLimit)
	}
	for _, pool := range pools {
		if err = requireOne(tx.ExecContext(ctx, s.q(`UPDATE budgets SET active=active-1,reserved=reserved-?,retained=retained+?,
tokens=tokens+?,reserved_tokens=reserved_tokens-?,incomplete_usages=incomplete_usages+?
WHERE id=? AND active>0 AND reserved>=? AND reserved_tokens>=?`),
			reserve, bodyBytes, reported, released, incomplete, pool.id, reserve, released)); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) releaseOutputBudgets(ctx context.Context, tx *sql.Tx, operation control.Operation) error {
	pools, err := s.budgetPools(ctx, tx, operation.View.SessionID, true)
	if err != nil {
		return err
	}
	reserve := kernel.OutputReservation(operation.OutputLimit)
	for _, pool := range pools {
		if err = requireOne(tx.ExecContext(ctx, s.q("UPDATE budgets SET reserved=reserved-? WHERE id=? AND reserved>=?"),
			reserve, pool.id, reserve)); err != nil {
			return err
		}
	}
	return nil
}

func (s *Store) Budget(ctx context.Context, session string) (out wire.SessionBudget, err error) {
	pools, err := s.budgetPools(ctx, s.db, session, false)
	if err != nil {
		return out, err
	}
	out.SessionID = session
	out.RemainingCalls, out.AvailableOperations, out.AvailableBytes = ^uint64(0), ^uint64(0), ^uint64(0)
	for _, pool := range pools {
		out.TokenUsagePending = out.TokenUsagePending || pool.held > 0
		out.RemainingCalls = min(out.RemainingCalls, uint64(max(0, pool.maxCalls-pool.calls)))
		out.AvailableOperations = min(out.AvailableOperations, uint64(max(0, pool.maxActive-pool.active)))
		if pool.paused != 0 || (pool.id != session && pool.generation != pool.admitted) {
			out.AvailableOperations = 0
		}
		out.AvailableBytes = min(out.AvailableBytes, uint64(max(0, pool.maxBytes-pool.retained-pool.reserved)))
		if pool.maxTokens >= 0 {
			available := max(0, pool.maxTokens-pool.tokens-pool.held)
			if out.AvailableTokens != nil {
				previous, _ := strconv.ParseInt(*out.AvailableTokens, 10, 64)
				available = min(available, previous)
			}
			out.AvailableTokens = kernel.Ptr(strconv.FormatInt(available, 10))
		}
		if pool.id == session {
			out.Calls, out.ActiveOperations = uint64(pool.calls), uint64(pool.active)
			out.ReservedOutputBytes, out.RetainedBytes = uint64(pool.reserved), uint64(pool.retained)
			out.ReportedTokens, out.ReservedTokens = strconv.FormatInt(pool.tokens, 10), strconv.FormatInt(pool.held, 10)
			out.IncompleteUsages = uint64(pool.incomplete)
		}
	}
	if out.ReportedTokens == "" {
		return out, kernel.Fail(wire.ErrorCodeCorrupt, "session has no independent budget pool")
	}
	return out, nil
}
