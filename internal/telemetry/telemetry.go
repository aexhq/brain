// Package telemetry configures bounded OpenTelemetry exporters and instruments.
package telemetry

import (
	"context"
	"errors"
	"os"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/exporters/otlp/otlpmetric/otlpmetrichttp"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
)

type Metrics struct {
	Requests     metric.Int64Counter
	Duration     metric.Float64Histogram
	Active       metric.Int64UpDownCounter
	ToolCalls    metric.Int64Counter
	ToolDuration metric.Float64Histogram
	Appended     metric.Int64Counter
}

func Setup(ctx context.Context) (*Metrics, func(context.Context) error, error) {
	var shutdowns []func(context.Context) error
	if os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT") != "" {
		res := resource.NewWithAttributes("", attribute.String("service.name", "brain"))
		te, err := otlptracehttp.New(ctx, otlptracehttp.WithTimeout(5*time.Second))
		if err != nil {
			return nil, nil, err
		}
		tp := sdktrace.NewTracerProvider(sdktrace.WithResource(res), sdktrace.WithBatcher(te, sdktrace.WithMaxQueueSize(2048), sdktrace.WithExportTimeout(5*time.Second)))
		otel.SetTracerProvider(tp)
		shutdowns = append(shutdowns, tp.Shutdown)
		me, err := otlpmetrichttp.New(ctx, otlpmetrichttp.WithTimeout(5*time.Second))
		if err != nil {
			tp.Shutdown(ctx)
			return nil, nil, err
		}
		mp := sdkmetric.NewMeterProvider(sdkmetric.WithResource(res), sdkmetric.WithReader(sdkmetric.NewPeriodicReader(me, sdkmetric.WithInterval(15*time.Second), sdkmetric.WithTimeout(5*time.Second))))
		otel.SetMeterProvider(mp)
		shutdowns = append(shutdowns, mp.Shutdown)
	}
	otel.SetTextMapPropagator(propagation.TraceContext{})
	meter := otel.Meter("github.com/aexhq/brain")
	requests, e1 := meter.Int64Counter("brain.http.requests")
	duration, e2 := meter.Float64Histogram("brain.http.duration", metric.WithUnit("s"))
	active, e3 := meter.Int64UpDownCounter("brain.executions.active")
	calls, e4 := meter.Int64Counter("brain.tool.calls")
	toolDuration, e5 := meter.Float64Histogram("brain.tool.duration", metric.WithUnit("s"))
	appended, e6 := meter.Int64Counter("brain.transcript.bytes", metric.WithUnit("By"))
	if err := errors.Join(e1, e2, e3, e4, e5, e6); err != nil {
		return nil, nil, err
	}
	close := func(ctx context.Context) error {
		var errs []error
		for _, f := range shutdowns {
			errs = append(errs, f(ctx))
		}
		return errors.Join(errs...)
	}
	return &Metrics{requests, duration, active, calls, toolDuration, appended}, close, nil
}
