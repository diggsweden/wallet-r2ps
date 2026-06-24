// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::TracerProvider;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

pub const DEFAULT_OTLP_ENDPOINT: &str = "http://gateway-collector.observability.svc:4317";

pub struct TelemetryGuard {
    _tracer_provider: TracerProvider,
    _meter_provider: SdkMeterProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        opentelemetry::global::shutdown_tracer_provider();
        // SdkMeterProvider flushes via its own Drop on the held field.
    }
}

pub fn init(service_name: &'static str) -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());

    // Register W3C tracecontext as the global propagator so incoming
    // `traceparent` headers (set by Istio envoy) can be extracted and
    // used as the parent of in-process spans. Without this every app
    // span starts a new trace, disjoint from the envoy parent.
    global::set_text_map_propagator(TraceContextPropagator::new());

    let resource = Resource::new(vec![KeyValue::new("service.name", service_name)]);

    // Traces: OTLP/gRPC → gateway-collector → TempoStack.
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()?;

    let tracer_provider = TracerProvider::builder()
        .with_batch_exporter(span_exporter, runtime::Tokio)
        .with_resource(resource.clone())
        .build();

    let tracer = tracer_provider.tracer(service_name);
    global::set_tracer_provider(tracer_provider.clone());

    // Metrics: OTLP/gRPC → gateway-collector → prometheus exporter on
    // :9464 → UWM scrape via the otel-collector ServiceMonitor. The
    // global meter provider lets any module emit counters/histograms via
    // `opentelemetry::global::meter("wallet-bff")` without holding refs.
    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()?;

    let reader = PeriodicReader::builder(metric_exporter, runtime::Tokio).build();

    let meter_provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build();

    global::set_meter_provider(meter_provider.clone());

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("wallet_bff=info,rdkafka=warn,redis=warn")),
        )
        .with(fmt::layer())
        .with(OpenTelemetryLayer::new(tracer))
        .init();

    Ok(TelemetryGuard {
        _tracer_provider: tracer_provider,
        _meter_provider: meter_provider,
    })
}
