// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::session_state_spi_port::SessionKey;
use crate::application::service::operations::OperationContext;
use crate::domain::{EcPublicJwk, OperationId};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ResponseContext {
    pub request_id: String,
    pub request_type: OperationId,
    pub session_key: Option<SessionKey>,
    pub ttl: Option<Duration>,
    /// Client JWE public key; Device-encrypted responses are encrypted to this key (ECDH-ES)
    pub device_jwe_public_key: EcPublicJwk,
}

#[derive(Debug)]
pub struct WorkerInput {
    pub operation_context: OperationContext,
    pub response_context: ResponseContext,
}
