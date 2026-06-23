// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! W3C tracecontext propagation over rdkafka message headers — consumer side.
//!
//! Adapter implementing `opentelemetry::propagation::Extractor` over an
//! optional `rdkafka::message::BorrowedHeaders`. Combined with a registered
//! `TraceContextPropagator` (see `telemetry::init`) this lets a consumer
//! pick up the `traceparent` header that the bff producer injected and
//! make its span a child of the parent trace.

use opentelemetry::propagation::Extractor;
use rdkafka::message::{BorrowedHeaders, Headers};

pub struct KafkaHeaderExtractor<'a>(pub Option<&'a BorrowedHeaders>);

impl<'a> Extractor for KafkaHeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        let hdrs = self.0?;
        for i in 0..hdrs.count() {
            let h = hdrs.get(i);
            if h.key == key {
                return h.value.and_then(|v| std::str::from_utf8(v).ok());
            }
        }
        None
    }

    fn keys(&self) -> Vec<&str> {
        match self.0 {
            Some(hdrs) => (0..hdrs.count()).map(|i| hdrs.get(i).key).collect(),
            None => Vec::new(),
        }
    }
}
