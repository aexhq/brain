import { context, metrics, propagation, trace, SpanStatusCode, type ContextManager, type MeterProvider, type TextMapPropagator, type TracerProvider } from '@opentelemetry/api';

/** Install application-owned OpenTelemetry providers before constructing clients.
 * The application owns exporter queues, resource attributes and shutdown. */
export function configureTelemetry(options: { tracerProvider?: TracerProvider; meterProvider?: MeterProvider; contextManager?: ContextManager; propagator?: TextMapPropagator }): void {
  if (options.tracerProvider && !trace.setGlobalTracerProvider(options.tracerProvider)) throw new Error('Tracer provider already registered');
  if (options.meterProvider && !metrics.setGlobalMeterProvider(options.meterProvider)) throw new Error('Meter provider already registered');
  if (options.contextManager && !context.setGlobalContextManager(options.contextManager)) throw new Error('Context manager already registered');
  if (options.propagator && !propagation.setGlobalPropagator(options.propagator)) throw new Error('Propagator already registered');
}

export const telemetry = { context, trace, metrics, propagation };

// Only operation/status are metric dimensions. URLs, credentials, user payloads
// and tenant/session identifiers never enter SDK telemetry.
export async function instrumentRequest<T>(operation: string, run: (headers: Record<string, string>) => Promise<T>): Promise<T> {
  const span = trace.getTracer('@aexhq/brain').startSpan(`brain.client.${operation}`);
  const headers: Record<string, string> = {};
  const ctx = trace.setSpan(context.active(), span);
  propagation.inject(ctx, headers);
  const sc = span.spanContext();
  if (trace.isSpanContextValid(sc)) headers.traceparent = `00-${sc.traceId}-${sc.spanId}-${sc.traceFlags.toString(16).padStart(2, '0')}`;
  const started = performance.now();
  let status = 'ok';
  try { return await run(headers); }
  catch (error) { status = 'error'; span.setStatus({ code: SpanStatusCode.ERROR }); throw error; }
  finally {
    metrics.getMeter('@aexhq/brain').createHistogram('brain.client.duration', { unit: 's' }).record((performance.now() - started) / 1000, { operation, status });
    span.end();
  }
}
