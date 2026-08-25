//! Process-level OpenTelemetry wiring shared by Brain and its trusted component workers.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::{Counter, Gauge, Histogram};
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

pub const OTLP_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
pub const OTLP_PROTOCOL_ENV: &str = "OTEL_EXPORTER_OTLP_PROTOCOL";

pub struct Guard {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
    logger: Option<SdkLoggerProvider>,
}

impl Guard {
    pub fn enabled(&self) -> bool {
        self.tracer.is_some()
    }

    pub fn shutdown(mut self) -> anyhow::Result<()> {
        shutdown(&mut self.logger, "logs")?;
        shutdown(&mut self.meter, "metrics")?;
        shutdown(&mut self.tracer, "traces")
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = shutdown(&mut self.logger, "logs");
        let _ = shutdown(&mut self.meter, "metrics");
        let _ = shutdown(&mut self.tracer, "traces");
    }
}

trait Shutdown {
    fn shutdown(&self) -> Result<(), impl std::fmt::Display>;
}

impl Shutdown for SdkTracerProvider {
    fn shutdown(&self) -> Result<(), impl std::fmt::Display> {
        SdkTracerProvider::shutdown(self)
    }
}

impl Shutdown for SdkMeterProvider {
    fn shutdown(&self) -> Result<(), impl std::fmt::Display> {
        SdkMeterProvider::shutdown(self)
    }
}

impl Shutdown for SdkLoggerProvider {
    fn shutdown(&self) -> Result<(), impl std::fmt::Display> {
        SdkLoggerProvider::shutdown(self)
    }
}

fn shutdown<T: Shutdown>(provider: &mut Option<T>, signal: &str) -> anyhow::Result<()> {
    let Some(provider) = provider.take() else {
        return Ok(());
    };
    provider
        .shutdown()
        .map_err(|error| anyhow::anyhow!("OpenTelemetry {signal} shutdown failed: {error}"))
}

pub fn install(service_name: &'static str) -> anyhow::Result<Guard> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,hyper=warn,opentelemetry=warn".into());
    let fmt = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let Some(endpoint) = optional_nonempty_env(OTLP_ENDPOINT_ENV)? else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt)
            .try_init()?;
        return Ok(Guard {
            tracer: None,
            meter: None,
            logger: None,
        });
    };
    validate_endpoint(&endpoint)?;
    match optional_nonempty_env(OTLP_PROTOCOL_ENV)?.as_deref() {
        None | Some("http/protobuf") => {}
        Some(_) => anyhow::bail!("{OTLP_PROTOCOL_ENV} must be http/protobuf"),
    }

    let resource = Resource::builder().with_service_name(service_name).build();
    let tracer = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .build()?,
        )
        .build();
    let meter = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_periodic_exporter(
            opentelemetry_otlp::MetricExporter::builder()
                .with_http()
                .build()?,
        )
        .build();
    let logger = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(
            opentelemetry_otlp::LogExporter::builder()
                .with_http()
                .build()?,
        )
        .build();

    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_tracer_provider(tracer.clone());
    global::set_meter_provider(meter.clone());
    let trace_layer = tracing_opentelemetry::layer().with_tracer(tracer.tracer(service_name));
    let log_layer = OpenTelemetryTracingBridge::new(&logger);
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt)
        .with(trace_layer)
        .with(log_layer)
        .try_init()?;
    Ok(Guard {
        tracer: Some(tracer),
        meter: Some(meter),
        logger: Some(logger),
    })
}

pub fn inject_current_trace() -> HashMap<String, String> {
    let context = tracing::Span::current().context();
    let mut carrier = HashMap::new();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut MapInjector(&mut carrier));
    });
    carrier
}

pub fn set_parent_from_trace(span: &tracing::Span, carrier: &HashMap<String, String>) {
    let parent =
        global::get_text_map_propagator(|propagator| propagator.extract(&MapExtractor(carrier)));
    let _ = span.set_parent(parent);
}

pub fn record_http_request(
    method: &str,
    route: &str,
    status: u16,
    elapsed: Duration,
    active_turns: usize,
) {
    static REQUESTS: OnceLock<Counter<u64>> = OnceLock::new();
    static DURATION: OnceLock<Histogram<f64>> = OnceLock::new();
    static ACTIVE_TURNS: OnceLock<Gauge<u64>> = OnceLock::new();
    let meter = global::meter("brain.http");
    let requests = REQUESTS.get_or_init(|| {
        meter
            .u64_counter("brain.http.server.request.count")
            .with_description("Completed Brain HTTP requests")
            .build()
    });
    let duration = DURATION.get_or_init(|| {
        meter
            .f64_histogram("brain.http.server.request.duration")
            .with_unit("s")
            .with_description("Brain HTTP request duration")
            .build()
    });
    let active = ACTIVE_TURNS.get_or_init(|| {
        meter
            .u64_gauge("brain.session.active_turns")
            .with_description("Currently active Brain turns")
            .build()
    });
    let attributes = [
        KeyValue::new("http.request.method", method.to_owned()),
        KeyValue::new("http.route", route.to_owned()),
        KeyValue::new("http.response.status_code", i64::from(status)),
    ];
    requests.add(1, &attributes);
    duration.record(elapsed.as_secs_f64(), &attributes);
    active.record(active_turns as u64, &[]);
}

fn optional_nonempty_env(name: &str) -> anyhow::Result<Option<String>> {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => anyhow::bail!("{name} is not UTF-8"),
        Ok(value) if value.is_empty() => anyhow::bail!("{name} cannot be empty"),
        Ok(value) => Ok(Some(value)),
    }
}

fn validate_endpoint(endpoint: &str) -> anyhow::Result<()> {
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
        anyhow::bail!("{OTLP_ENDPOINT_ENV} must be an http:// or https:// URL");
    }
    Ok(())
}

struct MapInjector<'a>(&'a mut HashMap<String, String>);

impl Injector for MapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
}

struct MapExtractor<'a>(&'a HashMap<String, String>);

impl Extractor for MapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_explicit_and_http_only() {
        assert!(validate_endpoint("http://collector:4318").is_ok());
        assert!(validate_endpoint("https://telemetry.example").is_ok());
        assert!(validate_endpoint("grpc://collector:4317").is_err());
        assert!(validate_endpoint("collector:4318").is_err());
    }

    #[test]
    fn empty_trace_carrier_is_harmless() {
        set_parent_from_trace(&tracing::info_span!("test"), &HashMap::new());
    }
}
