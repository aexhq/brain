// Package telemetry configures bounded OpenTelemetry exporters and propagation.
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
	"go.opentelemetry.io/otel/propagation"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
)

// SetupService installs the public trace propagation and exporter contract for
// one process. Service packages create their own instruments.
func SetupService(ctx context.Context, service string) (func(context.Context) error, error) {
	if service == "" {
		return nil, errors.New("telemetry service name is required")
	}
	var shutdowns []func(context.Context) error
	if os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT") != "" {
		res := resource.NewWithAttributes("", attribute.String("service.name", service))
		te, err := otlptracehttp.New(ctx, otlptracehttp.WithTimeout(5*time.Second))
		if err != nil {
			return nil, err
		}
		tp := sdktrace.NewTracerProvider(sdktrace.WithResource(res), sdktrace.WithBatcher(te, sdktrace.WithMaxQueueSize(2048), sdktrace.WithExportTimeout(5*time.Second)))
		otel.SetTracerProvider(tp)
		shutdowns = append(shutdowns, tp.Shutdown)
		me, err := otlpmetrichttp.New(ctx, otlpmetrichttp.WithTimeout(5*time.Second))
		if err != nil {
			tp.Shutdown(ctx)
			return nil, err
		}
		mp := sdkmetric.NewMeterProvider(sdkmetric.WithResource(res), sdkmetric.WithReader(sdkmetric.NewPeriodicReader(me, sdkmetric.WithInterval(15*time.Second), sdkmetric.WithTimeout(5*time.Second))))
		otel.SetMeterProvider(mp)
		shutdowns = append(shutdowns, mp.Shutdown)
	}
	otel.SetTextMapPropagator(propagation.TraceContext{})
	close := func(ctx context.Context) error {
		var errs []error
		for _, f := range shutdowns {
			errs = append(errs, f(ctx))
		}
		return errors.Join(errs...)
	}
	return close, nil
}
