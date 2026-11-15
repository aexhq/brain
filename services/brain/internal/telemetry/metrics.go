// Package telemetry owns Brain's metric instruments.
package telemetry

import (
	"context"
	"errors"
	shared "github.com/aexhq/brain/services/shared/telemetry"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
)

type Metrics struct {
	Requests      metric.Int64Counter
	Duration      metric.Float64Histogram
	Active        metric.Int64UpDownCounter
	ToolCalls     metric.Int64Counter
	ToolDuration  metric.Float64Histogram
	QueueDelay    metric.Float64Histogram
	FirstOutput   metric.Float64Histogram
	RunnerStartup metric.Float64Histogram
	NativeBytes   metric.Int64Counter
	SyncPending   metric.Int64Gauge
	SyncDuration  metric.Float64Histogram
	SyncFailures  metric.Int64Counter
	Rejected      metric.Int64Counter
}

func Setup(ctx context.Context) (*Metrics, func(context.Context) error, error) {
	shutdown, err := shared.SetupService(ctx, "brain")
	if err != nil {
		return nil, nil, err
	}
	meter := otel.Meter("github.com/aexhq/brain/services/brain")
	requests, e1 := meter.Int64Counter("brain.http.requests")
	duration, e2 := meter.Float64Histogram("brain.http.duration", metric.WithUnit("s"))
	active, e3 := meter.Int64UpDownCounter("brain.executions.active")
	calls, e4 := meter.Int64Counter("brain.tool.calls")
	toolDuration, e5 := meter.Float64Histogram("brain.tool.duration", metric.WithUnit("s"))
	queue, e7 := meter.Float64Histogram("brain.input.queue_delay", metric.WithUnit("s"))
	first, e8 := meter.Float64Histogram("brain.assistant.first_output", metric.WithUnit("s"))
	startup, e9 := meter.Float64Histogram("brain.runner.startup", metric.WithUnit("s"))
	native, e10 := meter.Int64Counter("brain.native.appended_bytes", metric.WithUnit("By"))
	pending, e11 := meter.Int64Gauge("brain.sync.pending")
	syncDuration, e12 := meter.Float64Histogram("brain.sync.duration", metric.WithUnit("s"))
	syncFailures, e13 := meter.Int64Counter("brain.sync.failures")
	rejected, e14 := meter.Int64Counter("brain.http.rejected")
	if err := errors.Join(e1, e2, e3, e4, e5, e7, e8, e9, e10, e11, e12, e13, e14); err != nil {
		return nil, nil, errors.Join(err, shutdown(ctx))
	}
	return &Metrics{Requests: requests, Duration: duration, Active: active, ToolCalls: calls, ToolDuration: toolDuration, QueueDelay: queue, FirstOutput: first, RunnerStartup: startup, NativeBytes: native, SyncPending: pending, SyncDuration: syncDuration, SyncFailures: syncFailures, Rejected: rejected}, shutdown, nil
}
