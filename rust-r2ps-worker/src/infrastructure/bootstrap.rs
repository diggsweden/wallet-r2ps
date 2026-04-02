use crate::application::port::outgoing::hsm_spi_port::HsmSpiPort;
use crate::application::service::StateInitService;
use crate::application::{WorkerPorts, WorkerService};
use crate::infrastructure::KafkaConfig;
use crate::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
use crate::infrastructure::adapters::outgoing::opaque_pake_adapter::OpaquePakeAdapter;
use crate::infrastructure::adapters::outgoing::session_state_memory_cache::SessionStateMemoryCache;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::config::load_pem_from_base64;
use crate::infrastructure::config::{jose_utils, key_derivation};
use crate::infrastructure::hsm_wrapper::HsmWrapper;
use crate::infrastructure::r2ps_response_kafka_message_sender::WorkerResponseKafkaSender;
use crate::infrastructure::state_init_response_kafka_sender::StateInitResponseKafkaMessageSender;
use p256::SecretKey;
use p256::pkcs8::DecodePrivateKey;
use std::sync::Arc;
use tracing::info;

pub fn build_services(
    app_config: &AppConfig,
    kafka_config: Arc<KafkaConfig>,
) -> (WorkerService, StateInitService) {
    let hsm = Arc::new(HsmWrapper::new(app_config.clone().into()).unwrap());

    let (jose_secret, opaque_secret) = if let Some(root_label) = &app_config.hsm_root_key_label {
        let jws_sep = app_config
            .jws_domain_separator
            .as_deref()
            .expect("JWS_DOMAIN_SEPARATOR required in HSM mode (e.g. \"rk-202501_jws-202501\")");
        let opaque_sep = app_config.opaque_domain_separator.as_deref().expect(
            "OPAQUE_DOMAIN_SEPARATOR required in HSM mode (e.g. \"rk-202501_opaque-202501\")",
        );
        assert!(
            jws_sep != opaque_sep,
            "JWS_DOMAIN_SEPARATOR and OPAQUE_DOMAIN_SEPARATOR must differ"
        );
        info!("Using HSM key derivation (root key: {})", root_label);
        (
            derive_key_from_hsm(hsm.as_ref(), root_label, jws_sep),
            derive_key_from_hsm(hsm.as_ref(), root_label, opaque_sep),
        )
    } else {
        info!("Using legacy PEM key config");
        let pem = load_pem_from_base64(
            app_config
                .server_private_key
                .as_deref()
                .expect("SERVER_PRIVATE_KEY required"),
        )
        .expect("Failed to load SERVER_PRIVATE_KEY");
        let pem_str = pem::encode(&pem);
        let secret = SecretKey::from_pkcs8_pem(&pem_str)
            .expect("Failed to parse server private key as P-256 PKCS8");
        // Legacy mode: same key for JWS and OPAQUE — preserves backwards compat
        // with existing client registrations. HSM mode derives separate keys.
        (secret.clone(), secret)
    };

    let jose = Arc::new(JoseAdapter::new(jose_secret).expect("Failed to initialize JoseAdapter"));
    // In legacy mode the KID is fixed (derived from SERVER_PRIVATE_KEY), so clients
    // need not include server_kid. In HSM mode the KID can rotate, so it is required.
    let legacy_key_mode = app_config.hsm_root_key_label.is_none();

    let (opaque_server_id, opaque_domain_separator) = if app_config.hsm_root_key_label.is_some() {
        // Key derivation mode: identify the server by its derived key KID.
        let kid = jose_utils::ec_kid_from_secret(&opaque_secret);
        // opaque_sep already validated above; safe to unwrap
        (
            kid.clone(),
            app_config.opaque_domain_separator.clone().unwrap(),
        )
    } else {
        // Legacy mode: use the configured identifier ("cloud-wallet.digg.se" etc.)
        // so that existing client registrations remain compatible.
        let id = app_config.opaque_server_identifier.clone();
        (id.clone(), id)
    };

    let pake = Arc::new(
        OpaquePakeAdapter::build(
            &opaque_secret,
            &app_config.opaque_server_setup,
            opaque_domain_separator,
            opaque_server_id.clone(),
            app_config.opaque_context.clone(),
        )
        .expect("Failed to build OPAQUE adapter"),
    );

    let ports = WorkerPorts {
        jose: jose.clone(),
        worker_response: Arc::new(WorkerResponseKafkaSender::new(&kafka_config)),
        session_state: Arc::new(SessionStateMemoryCache::new()),
        hsm,
        pake,
    };

    let worker_service = WorkerService::new(ports, legacy_key_mode);

    let state_init_response_sender =
        Arc::new(StateInitResponseKafkaMessageSender::new(&kafka_config));
    let state_init_service =
        StateInitService::new(state_init_response_sender, jose, opaque_server_id);

    (worker_service, state_init_service)
}

fn derive_key_from_hsm(hsm: &HsmWrapper, root_label: &str, domain_sep: &str) -> SecretKey {
    let hmac_output = hsm.derive_key(root_label, domain_sep).unwrap_or_else(|e| {
        panic!("HSM key derivation failed (root={root_label}, domain={domain_sep}): {e:?}")
    });
    key_derivation::derive_scalar(hmac_output.as_ref(), domain_sep).unwrap_or_else(|e| {
        panic!("Key scalar derivation failed (root={root_label}, domain={domain_sep}): {e:?}")
    })
}
