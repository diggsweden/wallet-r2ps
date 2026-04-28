// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::domain::StateInitResponse;

pub trait StateInitResponseSpiPort {
    fn send(
        &self,
        response: StateInitResponse,
        response_topic: &str,
    ) -> Result<(), StateInitResponseError>;
}

#[derive(Debug)]
pub enum StateInitResponseError {
    ConnectionError,
    SerializationError,
}
