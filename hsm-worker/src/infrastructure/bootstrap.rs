// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::jose_port::JoseError;
use crate::application::port::outgoing::self_test_spi_port::{
    CheckResult, Outcome, SelfTestError, TsfClaim,
};
use crate::application::service::StateInitService;
use crate::application::session_state_spi_port::SessionStateSpiPort;
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

/// A start-up check that failed. One variant per implicit check that `build_services`
/// performs, so a failure can be named and audited instead of only panicking.
#[derive(Debug)]
pub enum BootstrapError {
    /// Could not reach or open the HSM token.
    HsmConnect(String),
    /// Reached the token but could not authenticate to it.
    HsmLogin(String),
    /// Authenticated, but the AES wrapping key is not usable.
    WrapKeyMissing(String),
    /// A static PEM server key (legacy single-key SERVER_PRIVATE_KEY, or the two-key
    /// SERVER_JWS_PRIVATE_KEY/SERVER_JWE_PRIVATE_KEY/SERVER_ENCRYPTION_KEY set — used when
    /// no HSM root key is configured) is missing or not a valid P-256 PKCS#8 key.
    LegacyKeyConfig(String),
    /// Deriving a service key from the HSM root key failed.
    KeyDerivation { separator: String, cause: String },
    /// The JOSE adapter rejected the derived key.
    JoseInit(JoseError),
    /// The OPAQUE server setup could not be loaded or created.
    OpaqueInit(String),
}

impl From<BootstrapError> for CheckResult {
    fn from(value: BootstrapError) -> Self {
        let (name, claim, detail) = match value {
            BootstrapError::HsmConnect(detail) => {
                ("hsm_connect", TsfClaim::WscdHsmConnectivity, detail)
            }
            BootstrapError::HsmLogin(detail) => {
                ("hsm_login", TsfClaim::WscdHsmConnectivity, detail)
            }

            BootstrapError::WrapKeyMissing(detail) => (
                "hsm_wrap_key_missing",
                TsfClaim::WscdHsmConnectivity,
                detail,
            ),
            BootstrapError::LegacyKeyConfig(detail) => (
                "legacy_key_config",
                TsfClaim::CryptographicLibraries,
                detail,
            ),
            BootstrapError::KeyDerivation { separator, cause } => (
                "hsm_key_derivation",
                TsfClaim::WscdHsmConnectivity,
                format!("{separator}: {cause}"),
            ),
            BootstrapError::JoseInit(err) => (
                "jose_init",
                TsfClaim::CryptographicLibraries,
                format!("{err:?}"),
            ),
            BootstrapError::OpaqueInit(detail) => {
                ("opaque_init", TsfClaim::CryptographicLibraries, detail)
            }
        };

        CheckResult {
            name,
            claim,
            outcome: Outcome::Fail(SelfTestError { detail }),
        }
    }
}

/// Everything `build_services` produces. A struct rather than a tuple so later steps
/// can add fields without changing the signature again.
pub struct Services {
    pub worker: WorkerService,
    pub state_init: StateInitService,
    pub hsm: Arc<HsmWrapper>,
    pub session_state: Arc<dyn SessionStateSpiPort>,
}

pub fn build_services(
    app_config: &AppConfig,
    kafka_config: Arc<KafkaConfig>,
) -> Result<Services, BootstrapError> {
    let hsm = Arc::new(HsmWrapper::new(app_config.clone().into())?);

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
            let jws_sep = app_config.jws_domain_separator.as_deref().ok_or_else(|| {
                BootstrapError::KeyDerivation {
                    separator: "JWS_DOMAIN_SEPARATOR".to_owned(),
                    cause: "required in HSM mode (e.g. \"rk-202501_jws-202501\")".to_owned(),
                }
            })?;
            let opaque_sep = app_config
                .opaque_domain_separator
                .as_deref()
                .ok_or_else(|| BootstrapError::KeyDerivation {
                    separator: "OPAQUE_DOMAIN_SEPARATOR".to_owned(),
                    cause: "required in HSM mode (e.g. \"rk-202501_opaque-202501\")".to_owned(),
                })?;
            if jws_sep == opaque_sep {
                return Err(BootstrapError::KeyDerivation {
                    separator: "JWS_DOMAIN_SEPARATOR/OPAQUE_DOMAIN_SEPARATOR".to_owned(),
                    cause: "must be different values".to_owned(),
                });
            }
            info!("Using HSM key derivation (root key: {})", root_label);
            let jose_secret = derive_key_from_hsm(hsm.as_ref(), root_label, jws_sep)?;
            let jwe_secret = match app_config.jwe_domain_separator.as_deref() {
                Some(jwe_sep) => {
                    if jws_sep == jwe_sep {
                        return Err(BootstrapError::KeyDerivation {
                            separator: "JWS_DOMAIN_SEPARATOR/JWE_DOMAIN_SEPARATOR".to_owned(),
                            cause: "must be different values".to_owned(),
                        });
                    }
                    if jwe_sep == opaque_sep {
                        return Err(BootstrapError::KeyDerivation {
                            separator: "JWE_DOMAIN_SEPARATOR/OPAQUE_DOMAIN_SEPARATOR".to_owned(),
                            cause: "must be different values".to_owned(),
                        });
                    }
                    derive_key_from_hsm(hsm.as_ref(), root_label, jwe_sep)?
                }
                None => {
                    warn!(
                        "JWE_DOMAIN_SEPARATOR not set; using JWS key for JWE — key separation not enforced; set JWE_DOMAIN_SEPARATOR to fix"
                    );
                    jose_secret.clone()
                }
            };
            let opaque_secret = derive_key_from_hsm(hsm.as_ref(), root_label, opaque_sep)?;
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
        None => match app_config.server_jws_private_key.as_deref() {
            // Two-key PEM mode: explicit SERVER_JWS_PRIVATE_KEY + SERVER_JWE_PRIVATE_KEY.
            Some(jws_b64) => {
                info!("Using two-key PEM config (SERVER_JWS_PRIVATE_KEY + SERVER_JWE_PRIVATE_KEY)");
                let jws_pem = load_pem_from_base64(jws_b64).map_err(|e| {
                    BootstrapError::LegacyKeyConfig(format!("SERVER_JWS_PRIVATE_KEY: {e:?}"))
                })?;
                let jose_secret = SecretKey::from_pkcs8_pem(&pem::encode(&jws_pem)).map_err(
                    |_| {
                        BootstrapError::LegacyKeyConfig(
                            "SERVER_JWS_PRIVATE_KEY is not a P-256 PKCS#8 key".to_owned(),
                        )
                    },
                )?;
                let jwe_b64 = app_config.server_jwe_private_key.as_deref().ok_or_else(|| {
                    BootstrapError::LegacyKeyConfig(
                        "SERVER_JWE_PRIVATE_KEY required when SERVER_JWS_PRIVATE_KEY is set"
                            .to_owned(),
                    )
                })?;
                let jwe_pem = load_pem_from_base64(jwe_b64).map_err(|e| {
                    BootstrapError::LegacyKeyConfig(format!("SERVER_JWE_PRIVATE_KEY: {e:?}"))
                })?;
                let jwe_secret = SecretKey::from_pkcs8_pem(&pem::encode(&jwe_pem)).map_err(
                    |_| {
                        BootstrapError::LegacyKeyConfig(
                            "SERVER_JWE_PRIVATE_KEY is not a P-256 PKCS#8 key".to_owned(),
                        )
                    },
                )?;
                let id = app_config.opaque_server_identifier.clone();
                ModeConfig {
                    jose_secret: jose_secret.clone(),
                    jwe_secret,
                    opaque_secret: jose_secret,
                    opaque_server_id: id.clone(),
                    opaque_domain_separator: id,
                    legacy_key_mode: false,
                }
            }
            // Legacy single-key PEM mode: SERVER_PRIVATE_KEY used for JWS and OPAQUE;
            // SERVER_ENCRYPTION_KEY optionally provides a distinct JWE key.
            None => {
                info!("Using legacy single-key PEM config (SERVER_PRIVATE_KEY)");
                let encoded = app_config.server_private_key.as_deref().ok_or_else(|| {
                    BootstrapError::LegacyKeyConfig(
                        "one of SERVER_JWS_PRIVATE_KEY or SERVER_PRIVATE_KEY is required"
                            .to_owned(),
                    )
                })?;
                let pem = load_pem_from_base64(encoded).map_err(|e| {
                    BootstrapError::LegacyKeyConfig(format!("SERVER_PRIVATE_KEY: {e:?}"))
                })?;
                // The pkcs8 error is discarded rather than formatted: it comes from a parser
                // that has seen key bytes, and this string reaches a log line.
                let secret = SecretKey::from_pkcs8_pem(&pem::encode(&pem)).map_err(|_| {
                    BootstrapError::LegacyKeyConfig(
                        "SERVER_PRIVATE_KEY is not a P-256 PKCS#8 key".to_owned(),
                    )
                })?;
                let jwe_secret = match app_config.server_encryption_key.as_deref() {
                    Some(enc_b64) => {
                        let enc_pem = load_pem_from_base64(enc_b64).map_err(|e| {
                            BootstrapError::LegacyKeyConfig(format!(
                                "SERVER_ENCRYPTION_KEY: {e:?}"
                            ))
                        })?;
                        SecretKey::from_pkcs8_pem(&pem::encode(&enc_pem)).map_err(|_| {
                            BootstrapError::LegacyKeyConfig(
                                "SERVER_ENCRYPTION_KEY is not a P-256 PKCS#8 key".to_owned(),
                            )
                        })?
                    }
                    None => {
                        warn!(
                            "SERVER_ENCRYPTION_KEY not set; using SERVER_PRIVATE_KEY for JWE — key separation not enforced; use SERVER_JWS_PRIVATE_KEY + SERVER_JWE_PRIVATE_KEY for new deployments"
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
        },
    };

    let jose = Arc::new(
        JoseAdapter::new(mode.jose_secret, mode.jwe_secret).map_err(BootstrapError::JoseInit)?,
    );

    let pake = Arc::new(
        OpaquePakeAdapter::build(
            &mode.opaque_secret,
            &app_config.opaque_server_setup,
            mode.opaque_domain_separator,
            mode.opaque_server_id.clone(),
            app_config.opaque_context.clone(),
        )
        .map_err(BootstrapError::OpaqueInit)?,
    );

    let session_state = Arc::new(SessionStateMemoryCache::new());

    let ports = WorkerPorts {
        jose: jose.clone(),
        worker_response: Arc::new(WorkerResponseKafkaSender::new(&kafka_config)),
        session_state: session_state.clone(),
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
        hsm.clone(),
        app_config.hsm_key_label.clone(),
        mode.opaque_server_id,
    );

    Ok(Services {
        worker: worker_service,
        state_init: state_init_service,
        hsm,
        session_state,
    })
}

fn derive_key_from_hsm(
    hsm: &HsmWrapper,
    root_label: &str,
    domain_sep: &str,
) -> Result<SecretKey, BootstrapError> {
    let hmac_output =
        hsm.derive_key(root_label, domain_sep)
            .map_err(|e| BootstrapError::KeyDerivation {
                separator: domain_sep.to_owned(),
                cause: format!("HSM HMAC with root key {root_label:?} failed: {e}"),
            })?;
    key_derivation::derive_scalar(hmac_output.as_ref(), domain_sep).map_err(|e| {
        BootstrapError::KeyDerivation {
            separator: domain_sep.to_owned(),
            cause: e.to_string(),
        }
    })
}
