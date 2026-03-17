// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Client-side OPAQUE protocol operations (registration + login).
//!
//! Uses the same cipher suite as the server (rust-r2ps-worker) and the
//! Android/Swift access mechanism libraries:
//! - OPRF: NIST P-256
//! - Key Exchange: Triple-DH (P-256, SHA-256)
//! - KSF: Identity (no stretching — PIN stretching is done externally)

use anyhow::{Context, Result};
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialResponse, Identifiers, RegistrationResponse,
};
use rand::rngs::OsRng;

// ─── Cipher suite (must match server + Android/Swift) ───

#[derive(Clone, Copy)]
struct Cs;
impl CipherSuite for Cs {
    type OprfCs = p256::NistP256;
    type KeyExchange = opaque_ke::TripleDh<p256::NistP256, sha2::Sha256>;
    type Ksf = opaque_ke::ksf::Identity;
}

// ─── Registration ───

pub struct RegistrationStartResult {
    pub registration_request: Vec<u8>,
    pub client_registration: Vec<u8>,
}

pub struct RegistrationFinishResult {
    pub registration_upload: Vec<u8>,
    pub export_key: Vec<u8>,
}

/// Start client-side OPAQUE registration.
///
/// `password` should be the stretched PIN (output of `stretch_pin`).
pub fn client_registration_start(password: &[u8]) -> Result<RegistrationStartResult> {
    let mut rng = OsRng;
    let result = ClientRegistration::<Cs>::start(&mut rng, password)
        .map_err(|e| anyhow::anyhow!("registration start failed: {:?}", e))?;

    Ok(RegistrationStartResult {
        registration_request: result.message.serialize().to_vec(),
        client_registration: result.state.serialize().to_vec(),
    })
}

/// Finish client-side OPAQUE registration.
pub fn client_registration_finish(
    password: &[u8],
    client_registration: &[u8],
    registration_response: &[u8],
    client_identifier: Option<&[u8]>,
    server_identifier: Option<&[u8]>,
) -> Result<RegistrationFinishResult> {
    let mut rng = OsRng;

    let client_reg = ClientRegistration::<Cs>::deserialize(client_registration)
        .map_err(|e| anyhow::anyhow!("bad client_registration: {:?}", e))?;

    let reg_response = RegistrationResponse::<Cs>::deserialize(registration_response)
        .context("bad registration_response")?;

    let params = ClientRegistrationFinishParameters {
        identifiers: Identifiers {
            client: client_identifier,
            server: server_identifier,
        },
        ksf: None,
    };

    let result = client_reg
        .finish(&mut rng, password, reg_response, params)
        .map_err(|e| anyhow::anyhow!("registration finish failed: {:?}", e))?;

    Ok(RegistrationFinishResult {
        registration_upload: result.message.serialize().to_vec(),
        export_key: result.export_key.to_vec(),
    })
}

// ─── Login ───

pub struct LoginStartResult {
    pub credential_request: Vec<u8>,
    pub client_login_state: Vec<u8>,
}

#[allow(dead_code)]
pub struct LoginFinishResult {
    pub credential_finalization: Vec<u8>,
    pub session_key: Vec<u8>,
    pub export_key: Vec<u8>,
}

/// Start client-side OPAQUE login.
///
/// `password` should be the stretched PIN (output of `stretch_pin`).
pub fn client_login_start(password: &[u8]) -> Result<LoginStartResult> {
    let mut rng = OsRng;
    let result = ClientLogin::<Cs>::start(&mut rng, password)
        .map_err(|e| anyhow::anyhow!("login start failed: {:?}", e))?;

    Ok(LoginStartResult {
        credential_request: result.message.serialize().to_vec(),
        client_login_state: result.state.serialize().to_vec(),
    })
}

/// Finish client-side OPAQUE login, producing the session key.
pub fn client_login_finish(
    credential_response: &[u8],
    client_login_state: &[u8],
    password: &[u8],
    context: &[u8],
    client_identifier: &[u8],
    server_identifier: &[u8],
) -> Result<LoginFinishResult> {
    let mut rng = OsRng;

    let cred_resp = CredentialResponse::<Cs>::deserialize(credential_response)
        .map_err(|e| anyhow::anyhow!("bad credential_response: {:?}", e))?;

    let login_state = ClientLogin::<Cs>::deserialize(client_login_state)
        .map_err(|e| anyhow::anyhow!("bad client_login_state: {:?}", e))?;

    let params = ClientLoginFinishParameters {
        context: Some(context),
        identifiers: Identifiers {
            client: Some(client_identifier),
            server: Some(server_identifier),
        },
        ksf: Default::default(),
    };

    let result = login_state
        .finish(&mut rng, password, cred_resp, params)
        .map_err(|e| anyhow::anyhow!("login finish failed: {:?}", e))?;

    Ok(LoginFinishResult {
        credential_finalization: result.message.serialize().to_vec(),
        session_key: result.session_key.to_vec(),
        export_key: result.export_key.to_vec(),
    })
}
