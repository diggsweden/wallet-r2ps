use crate::application::{OpaqueConfig, WorkerPorts, WorkerService, load_pem_from_base64};
use crate::domain::ServiceRequestError;
use crate::infrastructure::KafkaConfig;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::hsm_wrapper::HsmWrapper;
use crate::infrastructure::pending_auth_memory_cache::PendingAuthMemoryCache;
use crate::infrastructure::r2ps_response_kafka_message_sender::WorkerResponseKafkaSender;
use crate::infrastructure::session_key_memory_cache::SessionKeyMemoryCache;
use josekit::jws::ES256;
use josekit::jws::alg::ecdsa::{EcdsaJwsSigner, EcdsaJwsVerifier};
use pem::Pem;
use std::sync::Arc;
use tracing::error;

pub fn build_services(
    app_config: &AppConfig,
    kafka_config: Arc<KafkaConfig>,
) -> (WorkerService, Arc<EcdsaJwsSigner>) {
    let server_public_key = load_pem_from_base64(&app_config.server_public_key)
        .expect("Failed to load SERVER_PUBLIC_KEY");
    let server_private_key = load_pem_from_base64(&app_config.server_private_key)
        .expect("Failed to load SERVER_PRIVATE_KEY");

    let ports = WorkerPorts {
        worker_response: Arc::new(WorkerResponseKafkaSender::new(&kafka_config)),
        session_key: Arc::new(SessionKeyMemoryCache::new()),
        hsm: Arc::new(HsmWrapper::new(app_config.clone().into()).unwrap()),
        pending_auth: Arc::new(PendingAuthMemoryCache::new()),
    };

    let opaque_config: OpaqueConfig = app_config.clone().into();

    let (jws_signer, state_jws_verifier) =
        jws_crypto_provider(&server_public_key, &server_private_key)
            .expect("Failed to initialize JWS crypto from server keys");
    let jws_signer = Arc::new(jws_signer);

    let worker_service = WorkerService::new(
        server_public_key,
        server_private_key,
        jws_signer.clone(),
        state_jws_verifier,
        ports,
        opaque_config,
    );

    (worker_service, jws_signer)
}

fn jws_crypto_provider(
    server_public_key: &Pem,
    server_private_key: &Pem,
) -> Result<(EcdsaJwsSigner, EcdsaJwsVerifier), ServiceRequestError> {
    // Create a signer from PEM
    let pem_string = pem::encode(server_private_key);
    let jws_signer = ES256.signer_from_pem(&pem_string).map_err(|e| {
        error!(
            "Failed to create signer from server private key PEM: {:?}",
            e
        );
        ServiceRequestError::JwsError
    })?;

    // Create a verifier from PEM
    let public_key_pem = pem::encode(server_public_key);
    let state_jws_verifier = ES256.verifier_from_pem(&public_key_pem).map_err(|e| {
        error!(
            "Failed to create verifier from server public key PEM: {:?}",
            e
        );
        ServiceRequestError::JwsError
    })?;
    Ok((jws_signer, state_jws_verifier))
}
