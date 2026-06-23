// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! W3C tracecontext propagation over rdkafka message headers — producer side.
//!
//! Adapter implementing `opentelemetry::propagation::Injector` so we can
//! attach `traceparent` (and any other propagator-emitted) headers to a
//! Kafka message. Combined with a registered `TraceContextPropagator`
//! (see `telemetry::init`), this lets a downstream consumer pick up the
//! current span's context and continue the trace.
//!
//! Usage:
//! ```ignore
//! let mut injector = KafkaHeaderInjector::default();
//! opentelemetry::global::get_text_map_propagator(|propagator| {
//!     propagator.inject_context(
//!         &tracing::Span::current().context(),
//!         &mut injector,
//!     );
//! });
//! let record = FutureRecord::to(topic).headers(injector.into_owned_headers());
//! ```

use opentelemetry::propagation::Injector;
use rdkafka::message::{Header, OwnedHeaders};

#[derive(Default)]
pub struct KafkaHeaderInjector {
    pairs: Vec<(String, String)>,
}

impl Injector for KafkaHeaderInjector {
    fn set(&mut self, key: &str, value: String) {
        self.pairs.push((key.to_string(), value));
    }
}

impl KafkaHeaderInjector {
    pub fn into_owned_headers(self) -> OwnedHeaders {
        let mut headers = OwnedHeaders::new();
        for (k, v) in &self.pairs {
            headers = headers.insert(Header {
                key: k,
                value: Some(v.as_bytes()),
            });
        }
        headers
    }
}
