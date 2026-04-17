// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod r2ps_response_consumer;
pub mod state_init_cache;
pub mod state_init_response_consumer;

/// Message key used by the BFF heartbeat producer. Consumers skip these messages.
pub const HEARTBEAT_KEY: &[u8] = b"__heartbeat__";
