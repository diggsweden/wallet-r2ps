// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! W3C tracecontext propagation over rdkafka message headers.
//!
//! Combined with a registered `TraceContextPropagator` (see
//! `telemetry::init`), the extractor lets a consumer adopt the parent
//! span from incoming `traceparent` headers, and the injector lets a
//! producer attach the current span's context onto outgoing messages
//! so downstream consumers can continue the trace.

use opentelemetry::propagation::{Extractor, Injector};
use rdkafka::message::{BorrowedHeaders, Header, Headers, OwnedHeaders};

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
