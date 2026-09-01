// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

pub const DEFAULT_OTLP_ENDPOINT: &str = "http://gateway-collector.observability.svc:4317";

pub struct TelemetryGuard {
    tracer_provider: SdkTracerProvider,
    _meter_provider: SdkMeterProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Err(err) = self.tracer_provider.shutdown() {
            tracing::warn!(?err, "Failed to shut down tracer provider");
        }
        // SdkMeterProvider flushes via its own Drop on the held field.
    }
}

pub fn init(service_name: &'static str) -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());

    // W3C tracecontext propagator so we can extract `traceparent` from
    // incoming Kafka message headers (set by wallet-bff's producer) and
    // make our consumer spans children of the bff request span.
    global::set_text_map_propagator(TraceContextPropagator::new());

    let resource = Resource::builder().with_service_name(service_name).build();

    // Traces: OTLP/gRPC → gateway-collector → TempoStack.
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()?;

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();

    let tracer = tracer_provider.tracer(service_name);
    global::set_tracer_provider(tracer_provider.clone());

    // Metrics: OTLP/gRPC → gateway-collector → prometheus exporter on
    // :9464 → UWM scrape via the otel-collector ServiceMonitor.
    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()?;

    let reader = PeriodicReader::builder(metric_exporter).build();

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build();

    global::set_meter_provider(meter_provider.clone());

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(
            fmt::layer()
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(false)
                .with_level(true),
        )
        .with(OpenTelemetryLayer::new(tracer))
        .init();

    Ok(TelemetryGuard {
        tracer_provider,
        _meter_provider: meter_provider,
    })
}
