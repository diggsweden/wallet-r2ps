// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod client_metadata;
pub mod error;
pub mod external;
pub mod hsm;
pub mod state_initialization;
pub mod typed_jwe;
pub mod typed_jws;

#[cfg(test)]
mod client_metadata_tests;

pub use client_metadata::*;
pub use error::ServiceRequestError;
pub use external::{HsmWorkerRequest, HsmWorkerResponse};
pub use hsm::*;
pub use state_initialization::*;
pub use typed_jwe::TypedJwe;
pub use typed_jws::TypedJws;

// Re-export shared wire types from hsm-common.
pub use hsm_common::{
    CreateKeyServiceData, CreateKeyServiceDataResponse, Curve, DeleteKeyServiceData, EncryptOption,
    InnerRequest, InnerResponse, KeyInfo, ListKeysRequest, ListKeysResponse, MessageVector,
    OperationId, OuterRequest, OuterResponse, PakePayloadVector, PakeRequest, PakeResponse,
    PakeState, SessionId, SignRequest, SignatureResponse, SignatureVector, Status,
};
