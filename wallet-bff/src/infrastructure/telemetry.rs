// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::TracerProvider;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

pub const DEFAULT_OTLP_ENDPOINT: &str = "http://gateway-collector.observability.svc:4317";

pub struct TelemetryGuard {
    _provider: TracerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        opentelemetry::global::shutdown_tracer_provider();
    }
}

pub fn init(service_name: &'static str) -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()?;

    let resource = Resource::new(vec![KeyValue::new("service.name", service_name)]);

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, runtime::Tokio)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer(service_name);
    global::set_tracer_provider(provider.clone());

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("wallet_bff=info,rdkafka=warn,redis=warn")),
        )
        .with(fmt::layer())
        .with(OpenTelemetryLayer::new(tracer))
        .init();

    Ok(TelemetryGuard {
        _provider: provider,
    })
}
