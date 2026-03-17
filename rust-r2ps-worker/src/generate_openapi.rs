use rust_r2ps_worker::domain;
use utoipa::OpenApi;

/// OpenAPI documentation for the HSM Worker domain model.
///
/// This struct registers all domain value object schemas so they can be
/// exported as an OpenAPI specification via `cargo run --bin generate_openapi`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "HSM Worker - Domain Model",
        version = "0.1.0",
        description = "Schema definitions for the HSM Worker domain model.\n\nThis document describes the data types used in the communication protocol between the BFF and the HSM Worker service, including request/response envelopes, PAKE authentication payloads, HSM key management types, and device state management."
    ),
    components(schemas(
        // HSM key types
        domain::EcPublicJwk,
        domain::HsmKey,
        domain::WrappedPrivateKey,
        // Protocol types
        domain::SessionId,
        domain::Status,
        domain::OperationId,
        domain::EncryptOption,
        domain::PakeState,
        domain::Curve,
        // Request/response envelopes
        domain::HsmWorkerRequestDto,
        domain::HsmWorkerRequest,
        domain::WorkerResponse,
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
        domain::WorkerRequestError,
        // State initialization
        domain::StateInitInnerRequest,
        domain::StateInitInnerResponse,
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
