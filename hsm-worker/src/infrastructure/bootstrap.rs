// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

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
use tracing::{info, warn};

pub fn build_services(
    app_config: &AppConfig,
    kafka_config: Arc<KafkaConfig>,
) -> (WorkerService, StateInitService) {
    let hsm = Arc::new(HsmWrapper::new(app_config.clone().into()).unwrap());

    struct ModeConfig {
        jose_secret: SecretKey,
        jwe_secret: SecretKey,
        opaque_secret: SecretKey,
        opaque_server_id: String,
        opaque_domain_separator: String,
        legacy_key_mode: bool,
    }

    let mode = match &app_config.hsm_root_key_label {
        // Key derivation mode: HSM key derivation is used to derive the JWS, JWE and OPAQUE secrets.
        Some(root_label) => {
            let jws_sep = app_config.jws_domain_separator.as_deref().expect(
                "JWS_DOMAIN_SEPARATOR required in HSM mode (e.g. \"rk-202501_jws-202501\")",
            );
            let opaque_sep = app_config.opaque_domain_separator.as_deref().expect(
                "OPAQUE_DOMAIN_SEPARATOR required in HSM mode (e.g. \"rk-202501_opaque-202501\")",
            );
            assert_ne!(
                jws_sep, opaque_sep,
                "JWS_DOMAIN_SEPARATOR and OPAQUE_DOMAIN_SEPARATOR must differ"
            );
            info!("Using HSM key derivation (root key: {})", root_label);
            let jose_secret = derive_key_from_hsm(hsm.as_ref(), root_label, jws_sep);
            let jwe_secret = match app_config.jwe_domain_separator.as_deref() {
                Some(jwe_sep) => {
                    assert_ne!(
                        jws_sep, jwe_sep,
                        "JWS_DOMAIN_SEPARATOR and JWE_DOMAIN_SEPARATOR must differ"
                    );
                    assert_ne!(
                        jwe_sep, opaque_sep,
                        "JWE_DOMAIN_SEPARATOR and OPAQUE_DOMAIN_SEPARATOR must differ"
                    );
                    derive_key_from_hsm(hsm.as_ref(), root_label, jwe_sep)
                }
                None => {
                    warn!(
                        "JWE_DOMAIN_SEPARATOR not set; using JWS key for JWE — key separation not enforced; set JWE_DOMAIN_SEPARATOR to fix"
                    );
                    jose_secret.clone()
                }
            };
            let opaque_secret = derive_key_from_hsm(hsm.as_ref(), root_label, opaque_sep);
            let opaque_server_id = jose_utils::ec_kid_from_secret(&opaque_secret);
            ModeConfig {
                jose_secret,
                jwe_secret,
                opaque_secret,
                opaque_server_id,
                opaque_domain_separator: opaque_sep.to_owned(),
                legacy_key_mode: false,
            }
        }
        // Legacy mode: server_private_key is used to derive the JWS and OPAQUE secrets;
        // server_encryption_key provides the distinct JWE decryption key.
        None => {
            info!("Using legacy PEM key config");
            let pem = load_pem_from_base64(
                app_config
                    .server_private_key
                    .as_deref()
                    .expect("SERVER_PRIVATE_KEY required"),
            )
            .expect("Failed to load SERVER_PRIVATE_KEY");
            let secret = SecretKey::from_pkcs8_pem(&pem::encode(&pem))
                .expect("Failed to parse server private key as P-256 PKCS8");
            let jwe_secret = match app_config.server_encryption_key.as_deref() {
                Some(enc_b64) => {
                    let enc_pem = load_pem_from_base64(enc_b64)
                        .expect("Failed to load SERVER_ENCRYPTION_KEY");
                    SecretKey::from_pkcs8_pem(&pem::encode(&enc_pem))
                        .expect("Failed to parse server encryption key as P-256 PKCS8")
                }
                None => {
                    warn!(
                        "SERVER_ENCRYPTION_KEY not set; using SERVER_PRIVATE_KEY for JWE — key separation not enforced; set SERVER_ENCRYPTION_KEY to fix"
                    );
                    secret.clone()
                }
            };
            // Legacy mode: same key for JWS and OPAQUE — preserves backwards compat
            // with existing client registrations.
            let id = app_config.opaque_server_identifier.clone();
            ModeConfig {
                jose_secret: secret.clone(),
                jwe_secret,
                opaque_secret: secret,
                opaque_server_id: id.clone(),
                opaque_domain_separator: id,
                legacy_key_mode: true,
            }
        }
    };

    let jose = Arc::new(
        JoseAdapter::new(mode.jose_secret, mode.jwe_secret)
            .expect("Failed to initialize JoseAdapter (signing and encryption keys must differ)"),
    );

    let pake = Arc::new(
        OpaquePakeAdapter::build(
            &mode.opaque_secret,
            &app_config.opaque_server_setup,
            mode.opaque_domain_separator,
            mode.opaque_server_id.clone(),
            app_config.opaque_context.clone(),
        )
        .expect("Failed to build OPAQUE adapter"),
    );

    let ports = WorkerPorts {
        jose: jose.clone(),
        worker_response: Arc::new(WorkerResponseKafkaSender::new(&kafka_config)),
        session_state: Arc::new(SessionStateMemoryCache::new()),
        hsm: hsm.clone(),
        pake,
    };

    let worker_service = WorkerService::new(
        ports,
        app_config.hsm_key_label.clone(),
        mode.legacy_key_mode,
    );

    let state_init_response_sender =
        Arc::new(StateInitResponseKafkaMessageSender::new(&kafka_config));
    let state_init_service = StateInitService::new(
        state_init_response_sender,
        jose,
        hsm,
        app_config.hsm_key_label.clone(),
        mode.opaque_server_id,
    );

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
