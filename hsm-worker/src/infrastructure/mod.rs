// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod adapters;

pub mod bootstrap;
pub mod config;
pub mod kafka_propagation;
pub mod telemetry;

pub use adapters::*;
pub use config::app_config::*;
pub use config::kafka::KafkaConfig;
