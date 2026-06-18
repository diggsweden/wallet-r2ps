// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Shared library backing the `integration-load-tests` binary and any sibling
//! load-test tool that wants to drive the wallet stack with real OPAQUE +
//! JWS/JWE traffic.
//!
//! The high-level [`client::access_mechanism::AccessMechanismClient`] is
//! generic over the [`backend::BackendClient`] trait, so a sibling crate can
//! plug in a Kafka transport while reusing all crypto and protocol code.

pub mod backend;
pub mod client;
pub mod crypto;
pub mod model;
pub mod protocol;
pub mod stats;
