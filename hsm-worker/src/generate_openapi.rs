// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use hsm_worker::domain;
use utoipa::OpenApi;

/// OpenAPI documentation for the R2PS HSM Worker domain model.
///
/// This struct registers all domain value object schemas so they can be
/// exported as an OpenAPI specification via `cargo run --bin generate_openapi`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "R2PS HSM Worker - Domain Model",
        version = "0.1.0",
        description = "Schema definitions for the R2PS (Remote to Phone Signing) HSM Worker domain model.\n\nThis document describes the data types used in the communication protocol between the R2PS REST API and the HSM Worker service, including request/response envelopes, PAKE authentication payloads, HSM key management types, and device state management."
    ),
    components(schemas(
        // HSM key types
        domain::EcPublicJwk,
        domain::HsmKey,
        domain::WrappedPrivateKey,
        // R2PS protocol types
        domain::SessionId,
        domain::Status,
        domain::OperationId,
        domain::EncryptOption,
        domain::PakeState,
        domain::Curve,
        // Request/response envelopes
        domain::HsmWorkerRequest,
        domain::OuterRequest,
        domain::OuterResponse,
        domain::InnerRequest,
        domain::InnerResponse,
        // PAKE types
        domain::PakeRequest,
        domain::PakeResponse,
        // HSM operation types
        domain::CreateKeyServiceData,
        domain::CreateKeyServiceDataResponse,
        domain::DeleteKeyServiceData,
        domain::SignRequest,
        domain::SignatureResponse,
        domain::ListKeysRequest,
        domain::ListKeysResponse,
        domain::KeyInfo,
        // Error types
        domain::ServiceRequestError,
        // State initialization
        domain::StateInitRequest,
        domain::StateInitResponse,
        // Client/device state
        domain::DeviceHsmState,
        domain::DeviceKeyEntry,
        domain::PasswordFileEntry,
        domain::PasswordFile,
    ))
)]
struct ApiDoc;

fn main() {
    let openapi = ApiDoc::openapi();
    println!("{}", openapi.to_pretty_json().unwrap());
}
