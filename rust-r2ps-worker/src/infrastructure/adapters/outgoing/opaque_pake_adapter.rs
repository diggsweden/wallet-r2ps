use crate::application::port::outgoing::pake_port;
use crate::application::port::outgoing::session_state_spi_port::{PendingLoginState, SessionKey};
use crate::domain;
use crate::domain::value_objects::client_metadata::PasswordFileEntry;
use crate::domain::value_objects::r2ps::PakePayloadVector;
use argon2::password_hash::rand_core::OsRng;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use opaque_ke;
use p256::NistP256;
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
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
    /// Domain separator identifying this OPAQUE server keypair (stored in PasswordFileEntry)
    opaque_domain_separator: String,
    /// KID of the OPAQUE server public key (sent to client in registrationStart response)
    opaque_server_id: String,
}

impl OpaquePakeAdapter {
    /// Build an adapter from a P-256 secret key and an optional persisted `ServerSetup`.
    ///
    /// # Why `opaque_server_setup` is required in production (both modes)
    ///
    /// OPAQUE `ServerSetup` contains two independent components:
    ///   1. The server keypair — used in the AKE handshake. In HSM key derivation mode
    ///      this is deterministically derived from the root key; in legacy mode it comes
    ///      from `SERVER_PRIVATE_KEY`. Either way it is stable across restarts.
    ///   2. An OPRF key — used to blind/unblind the password during registration and
    ///      authentication. This key is randomly generated and is NOT derivable from the
    ///      server keypair.
    ///
    /// If the OPRF key changes between restarts every existing client registration becomes
    /// permanently invalid (authentication will always fail). Therefore:
    ///   - On the first startup, the service logs `OPAQUE_SERVER_SETUP=<base64>`.
    ///   - That value **must** be saved and set in the environment for all subsequent starts.
    ///   - This requirement applies to both legacy PEM mode and HSM key derivation mode.
    pub fn build(
        secret_key: &SecretKey,
        opaque_server_setup: &Option<String>,
        opaque_domain_separator: String,
        opaque_server_id: String,
        context: String,
    ) -> Result<Self, String> {
        let server_setup = load_or_create_server_setup(opaque_server_setup, secret_key)?;
        Ok(Self {
            server_setup,
            context,
            opaque_domain_separator,
            opaque_server_id,
        })
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
            opaque_domain_separator: self.opaque_domain_separator.clone(),
        })
    }

    fn authentication_start(
        &self,
        request_bytes: &[u8],
        password_file_entry: &PasswordFileEntry,
        client_id: &str,
    ) -> Result<(PakePayloadVector, PendingLoginState), pake_port::PakeError> {
        if password_file_entry.opaque_domain_separator != self.opaque_domain_separator {
            warn!(
                "authentication_start: opaque_domain_separator mismatch: stored='{}', current='{}'",
                password_file_entry.opaque_domain_separator, self.opaque_domain_separator
            );
            return Err(pake_port::PakeError::InvalidRequest);
        }

        let password_file = opaque_ke::ServerRegistration::<DefaultCipherSuite>::deserialize(
            password_file_entry.password_file.as_bytes(),
        )
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
            Self::server_login_parameters(&self.context, client_id, &self.opaque_server_id);

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
            Self::server_login_parameters(&self.context, client_id, &self.opaque_server_id);

        let result = server_login.finish(finalization, params).map_err(|e| {
            warn!("authentication finish failed: {:?}", e);
            pake_port::PakeError::AuthFinishFailed
        })?;

        Ok(SessionKey::new(result.session_key.to_vec()))
    }
}

fn load_or_create_server_setup(
    opaque_server_setup: &Option<String>,
    secret_key: &SecretKey,
) -> Result<opaque_ke::ServerSetup<DefaultCipherSuite>, String> {
    match load_server_setup(opaque_server_setup) {
        Ok(setup) => Ok(setup),
        Err(_) => {
            let setup = create_server_setup(secret_key)?;
            info!(
                "OPAQUE_SERVER_SETUP={}",
                BASE64_STANDARD.encode(setup.serialize())
            );
            Ok(setup)
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

/// Build an OPAQUE ServerSetup from a P-256 secret key with a fresh random OPRF seed.
fn create_server_setup(
    secret_key: &SecretKey,
) -> Result<opaque_ke::ServerSetup<DefaultCipherSuite>, String> {
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
