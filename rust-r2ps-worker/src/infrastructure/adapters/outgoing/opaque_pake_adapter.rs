use crate::application::port::outgoing::pake_port;
use crate::application::port::outgoing::session_state_spi_port::{PendingLoginState, SessionKey};
use crate::domain;
use crate::domain::value_objects::r2ps::PakePayloadVector;
use argon2::password_hash::rand_core::OsRng;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use opaque_ke;
use p256::NistP256;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use pem::Pem;
use tracing::{debug, info, warn};

#[derive(Clone, Copy)]
struct DefaultCipherSuite;

impl opaque_ke::CipherSuite for DefaultCipherSuite {
    type OprfCs = NistP256;
    type KeyExchange = opaque_ke::TripleDh<NistP256, sha2::Sha256>;
    type Ksf = opaque_ke::ksf::Identity;
}

pub struct OpaquePakeAdapter {
    server_setup: opaque_ke::ServerSetup<DefaultCipherSuite>,
    context: String,
    server_identifier: String,
}

impl OpaquePakeAdapter {
    fn new(
        server_setup: opaque_ke::ServerSetup<DefaultCipherSuite>,
        context: String,
        server_identifier: String,
    ) -> Self {
        Self {
            server_setup,
            context,
            server_identifier,
        }
    }

    pub fn from_config(
        opaque_server_setup: &Option<String>,
        server_private_key: &Pem,
        context: String,
        server_identifier: String,
    ) -> Self {
        let server_setup = load_or_create_server_setup(opaque_server_setup, server_private_key);
        Self::new(server_setup, context, server_identifier)
    }

    fn server_login_parameters<'a>(
        context: &'a str,
        client_id: &'a str,
        server_identifier: &'a str,
    ) -> opaque_ke::ServerLoginParameters<'a, 'a> {
        let context_bytes = context.as_bytes();
        let client = client_id.as_bytes();
        let server = server_identifier.as_bytes();
        debug!(
            "OPAQUE context: '{}' hex: {}",
            String::from_utf8_lossy(context_bytes),
            hex::encode(context_bytes)
        );
        debug!(
            "OPAQUE client: '{}' hex: {}",
            String::from_utf8_lossy(client),
            hex::encode(client)
        );
        debug!(
            "OPAQUE server: '{}' hex: {}",
            String::from_utf8_lossy(server),
            hex::encode(server)
        );
        opaque_ke::ServerLoginParameters {
            context: Some(context_bytes),
            identifiers: opaque_ke::Identifiers {
                client: Some(client),
                server: Some(server),
            },
        }
    }
}

impl pake_port::PakePort for OpaquePakeAdapter {
    fn registration_start(
        &self,
        request_bytes: &[u8],
        client_id: &str,
    ) -> Result<PakePayloadVector, pake_port::PakeError> {
        let registration_request = opaque_ke::RegistrationRequest::deserialize(request_bytes)
            .map_err(|e| {
                warn!("invalid registration request: {:?}", e);
                pake_port::PakeError::InvalidRequest
            })?;

        debug!("Using client id for OPAQUE registration: {}", client_id);

        let result = opaque_ke::ServerRegistration::<DefaultCipherSuite>::start(
            &self.server_setup,
            registration_request,
            client_id.as_bytes(),
        )
        .map_err(|e| {
            warn!("registration start failed: {:?}", e);
            pake_port::PakeError::RegistrationStartFailed
        })?;

        Ok(PakePayloadVector::new(result.message.serialize().to_vec()))
    }

    fn registration_finish(
        &self,
        upload_bytes: &[u8],
    ) -> Result<pake_port::RegistrationResult, pake_port::PakeError> {
        let upload = opaque_ke::RegistrationUpload::<DefaultCipherSuite>::deserialize(upload_bytes)
            .map_err(|e| {
                warn!("invalid registration upload: {:?}", e);
                pake_port::PakeError::InvalidRequest
            })?;

        let password_file = opaque_ke::ServerRegistration::<DefaultCipherSuite>::finish(upload);
        Ok(pake_port::RegistrationResult {
            password_file: domain::PasswordFile(password_file.serialize().to_vec()),
            server_identifier: self.server_identifier.clone(),
        })
    }

    fn authentication_start(
        &self,
        request_bytes: &[u8],
        password_file_bytes: &[u8],
        client_id: &str,
    ) -> Result<(PakePayloadVector, PendingLoginState), pake_port::PakeError> {
        let password_file =
            opaque_ke::ServerRegistration::<DefaultCipherSuite>::deserialize(password_file_bytes)
                .map_err(|e| {
                warn!("invalid password file: {:?}", e);
                pake_port::PakeError::InvalidPasswordFile
            })?;

        let credential_request =
            opaque_ke::CredentialRequest::deserialize(request_bytes).map_err(|e| {
                warn!("invalid credential request: {:?}", e);
                pake_port::PakeError::InvalidRequest
            })?;

        debug!("Using client id for OPAQUE authentication: {}", client_id);

        let params =
            Self::server_login_parameters(&self.context, client_id, &self.server_identifier);

        let result = opaque_ke::ServerLogin::start(
            &mut OsRng,
            &self.server_setup,
            Some(password_file),
            credential_request,
            client_id.as_bytes(),
            params,
        )
        .map_err(|e| {
            warn!("authentication start failed: {:?}", e);
            pake_port::PakeError::AuthStartFailed
        })?;

        let response = PakePayloadVector::new(result.message.serialize().to_vec());
        let pending_state = PendingLoginState::new(result.state.serialize().to_vec());

        Ok((response, pending_state))
    }

    fn authentication_finish(
        &self,
        finalization_bytes: &[u8],
        pending_state: &PendingLoginState,
        client_id: &str,
    ) -> Result<SessionKey, pake_port::PakeError> {
        let server_login =
            opaque_ke::ServerLogin::<DefaultCipherSuite>::deserialize(pending_state.as_ref())
                .map_err(|e| {
                    warn!("invalid pending login state: {:?}", e);
                    pake_port::PakeError::InvalidRequest
                })?;

        let finalization = opaque_ke::CredentialFinalization::deserialize(finalization_bytes)
            .map_err(|e| {
                warn!("invalid credential finalization: {:?}", e);
                pake_port::PakeError::InvalidRequest
            })?;

        let params =
            Self::server_login_parameters(&self.context, client_id, &self.server_identifier);

        let result = server_login.finish(finalization, params).map_err(|e| {
            warn!("authentication finish failed: {:?}", e);
            pake_port::PakeError::AuthFinishFailed
        })?;

        Ok(SessionKey::new(result.session_key.to_vec()))
    }
}

fn load_or_create_server_setup(
    opaque_server_setup: &Option<String>,
    server_private_key: &Pem,
) -> opaque_ke::ServerSetup<DefaultCipherSuite> {
    match load_server_setup(opaque_server_setup) {
        Ok(setup) => setup,
        Err(_) => {
            let setup = create_server_setup(server_private_key)
                .expect("Failed to create OPAQUE server setup");
            info!(
                "OPAQUE_SERVER_SETUP={}",
                BASE64_STANDARD.encode(setup.serialize())
            );
            setup
        }
    }
}

fn load_server_setup(
    server_setup: &Option<String>,
) -> Result<opaque_ke::ServerSetup<DefaultCipherSuite>, String> {
    match server_setup {
        Some(encoded) => {
            let bytes = BASE64_STANDARD
                .decode(encoded.as_bytes())
                .map_err(|e| format!("Failed to decode server setup: {}", e))?;
            opaque_ke::ServerSetup::deserialize(&bytes)
                .map_err(|e| format!("Failed to deserialize server setup: {}", e))
        }
        None => Err("No server setup configured".to_string()),
    }
}

fn create_server_setup(
    server_private_key_pem: &Pem,
) -> Result<opaque_ke::ServerSetup<DefaultCipherSuite>, String> {
    let secret_key = p256::SecretKey::from_pkcs8_pem(&pem::encode(server_private_key_pem))
        .map_err(|e| format!("Failed to parse P-256 private key: {:?}", e))?;

    let keypair = opaque_ke::keypair::KeyPair::new(
        opaque_ke::keypair::PrivateKey::<NistP256>::deserialize(&secret_key.to_bytes())
            .map_err(|e| format!("Failed to deserialize private key: {:?}", e))?,
        opaque_ke::keypair::PublicKey::<NistP256>::deserialize(
            secret_key
                .public_key()
                .as_affine()
                .to_encoded_point(true)
                .as_bytes(),
        )
        .map_err(|e| format!("Failed to deserialize public key: {:?}", e))?,
    );

    Ok(opaque_ke::ServerSetup::<DefaultCipherSuite>::new_with_key_pair(&mut OsRng, keypair))
}
