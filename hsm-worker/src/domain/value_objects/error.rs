// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use base64::DecodeError;
use serde::{Deserialize, Serialize};
use std::string::FromUtf8Error;
use strum_macros::Display;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Errors that can occur during service request processing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum ServiceRequestError {
    /// JWS signature verification or encoding failed
    JwsError,
    /// JWE encryption or decryption failed
    JweError,
    /// The PAKE request payload is malformed
    InvalidPakeRequest,
    /// The registration request is invalid
    InvalidRegistrationRequest,
    /// OPAQUE server registration start failed
    ServerRegistrationStartFailed,
    /// OPAQUE server login start failed
    ServerLoginStartFailed,
    /// OPAQUE server login finish failed
    ServerLoginFinishFailed,
    /// Failed to serialize the response
    SerializeResponseError,
    /// Failed to serialize the device state
    SerializeStateError,
    /// The service request format is invalid
    InvalidServiceRequestFormat,
    /// The stored password file could not be deserialized
    InvalidSerializedPasswordFile,
    /// The authentication request is invalid
    InvalidAuthenticateRequest,
    /// The referenced key was not found
    UnknownKey,
    /// The session does not exist or has expired
    UnknownSession,
    /// The client/device is not recognized
    UnknownClient,
    /// The client's public key is invalid
    InvalidClientPublicKey,
    /// A public key parameter is invalid
    InvalidPublicKey,
    /// A key with the same identifier already exists
    DuplicateKey,
    /// The referenced HSM key was not found
    HsmKeyNotFound,
    /// The request context is not supported
    UnsupportedContext,
    /// The operation is not permitted in the current session state
    InvalidOperation,
    /// An internal server error occurred
    InternalServerError,
    InvalidAuthorizationCode,
    UnknownServerKid,
    Unknown,
}

impl From<DecodeError> for ServiceRequestError {
    fn from(_: DecodeError) -> Self {
        ServiceRequestError::JweError
    }
}

impl From<FromUtf8Error> for ServiceRequestError {
    fn from(_: FromUtf8Error) -> Self {
        ServiceRequestError::JweError
    }
}
