// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! High-level access mechanism client matching the Android OpaqueClient API.
//! Combines OPAQUE crypto + JOSE envelope construction + REST client.

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use josekit::jwk::Jwk;
use std::sync::Arc;

use crate::crypto::{opaque_client, pin_stretch};
use crate::protocol::message_builder::{build_pake_request_jws, build_session_request_jws};
use crate::protocol::response_parser::{unwrap_pake_response, unwrap_session_response};
use crate::protocol::types::{
    BffNewStateRequest, CreateKeyServiceData, CreateKeyServiceDataResponse, Curve, EcPublicJwk,
    MessageVector, OperationId, PakePayloadVector, PakeRequest, SignRequest,
};

use super::rest_client::RestClient;

pub struct AccessMechanismClient {
    rest: Arc<RestClient>,
    server_public_key: Jwk,
    device_private_key: Jwk,
    kid: String,
    pin_stretch_d: String,
    opaque_context: String,
    opaque_server_identifier: String,
}

impl AccessMechanismClient {
    pub fn new(
        rest: Arc<RestClient>,
        server_public_key: Jwk,
        device_private_key: Jwk,
        kid: String,
        pin_stretch_d: String,
        opaque_context: String,
        opaque_server_identifier: String,
    ) -> Self {
        Self {
            rest,
            server_public_key,
            device_private_key,
            kid,
            pin_stretch_d,
            opaque_context,
            opaque_server_identifier,
        }
    }

    /// Initialize device state via the BFF.
    /// Returns (client_id, authorization_code).
    pub async fn init_state(
        &self,
        public_key: &EcPublicJwk,
        ttl: &str,
    ) -> Result<(String, String)> {
        let request = BffNewStateRequest {
            public_key: public_key.clone(),
            ttl: Some(ttl.to_string()),
        };

        let resp = self.rest.create_device_state(&request).await?;
        let client_id = resp.client_id;
        let auth_code = resp
            .dev_authorization_code
            .context("No authorization code in device-states response")?;

        Ok((client_id, auth_code))
    }

    /// Register a PIN (two-round OPAQUE registration).
    /// Returns the export key.
    pub async fn register_pin(
        &self,
        pin: &str,
        client_id: &str,
        auth_code: &str,
    ) -> Result<Vec<u8>> {
        let stretched = pin_stretch::stretch_pin(pin, &self.pin_stretch_d)?;

        // Registration start
        let reg_start = opaque_client::client_registration_start(&stretched)?;
        let pake_req = PakeRequest {
            authorization: Some(auth_code.to_string()),
            purpose: None,
            data: PakePayloadVector::new(reg_start.registration_request),
        };
        let jws = build_pake_request_jws(
            OperationId::RegisterStart,
            &pake_req,
            None,
            0,
            &self.server_public_key,
            &self.device_private_key,
            &self.kid,
        )?;
        let resp = self.rest.submit_request(client_id, &jws).await?;
        if resp.status != "complete" {
            bail!("Registration start failed: {:?}", resp);
        }
        let result_jws = resp
            .result
            .context("No result in registration start response")?;

        // Extract OPAQUE response bytes
        let pake_resp = unwrap_pake_response(&result_jws, &self.device_private_key)?;
        let opaque_bytes = pake_resp
            .data
            .context("No OPAQUE data in registration start response")?;

        // Registration finish
        let client_id_bytes = self.kid.as_bytes();
        let server_id_bytes = self.opaque_server_identifier.as_bytes();
        let reg_finish = opaque_client::client_registration_finish(
            &stretched,
            &reg_start.client_registration,
            &opaque_bytes,
            Some(client_id_bytes),
            Some(server_id_bytes),
        )?;

        let pake_req = PakeRequest {
            authorization: Some(auth_code.to_string()),
            purpose: None,
            data: PakePayloadVector::new(reg_finish.registration_upload),
        };
        let jws = build_pake_request_jws(
            OperationId::RegisterFinish,
            &pake_req,
            None,
            1,
            &self.server_public_key,
            &self.device_private_key,
            &self.kid,
        )?;
        let resp = self.rest.submit_request(client_id, &jws).await?;
        if resp.status != "complete" {
            bail!("Registration finish failed: {:?}", resp);
        }

        Ok(reg_finish.export_key)
    }

    /// Create a session (two-round OPAQUE login).
    /// Returns (session_key, session_id).
    pub async fn create_session(&self, pin: &str, client_id: &str) -> Result<(Vec<u8>, String)> {
        let stretched = pin_stretch::stretch_pin(pin, &self.pin_stretch_d)?;

        // Login start
        let login_start = opaque_client::client_login_start(&stretched)?;
        let pake_req = PakeRequest {
            authorization: None,
            purpose: None,
            data: PakePayloadVector::new(login_start.credential_request),
        };
        let jws = build_pake_request_jws(
            OperationId::AuthenticateStart,
            &pake_req,
            None,
            1,
            &self.server_public_key,
            &self.device_private_key,
            &self.kid,
        )?;
        let resp = self.rest.submit_request(client_id, &jws).await?;
        if resp.status != "complete" {
            bail!("Login start failed: {:?}", resp);
        }
        let result_jws = resp.result.context("No result in login start response")?;

        // Extract OPAQUE response + session_id
        let pake_resp = unwrap_pake_response(&result_jws, &self.device_private_key)?;
        let opaque_bytes = pake_resp
            .data
            .context("No OPAQUE data in login start response")?;
        let session_id = pake_resp
            .session_id
            .context("No session_id in login start response")?;

        // Login finish
        let ctx = self.opaque_context.as_bytes();
        let client_id_bytes = self.kid.as_bytes();
        let server_id_bytes = self.opaque_server_identifier.as_bytes();
        let login_finish = opaque_client::client_login_finish(
            &opaque_bytes,
            &login_start.client_login_state,
            &stretched,
            ctx,
            client_id_bytes,
            server_id_bytes,
        )?;

        let pake_req = PakeRequest {
            authorization: None,
            purpose: Some("general".to_string()),
            data: PakePayloadVector::new(login_finish.credential_finalization),
        };
        let jws = build_pake_request_jws(
            OperationId::AuthenticateFinish,
            &pake_req,
            Some(&session_id),
            2,
            &self.server_public_key,
            &self.device_private_key,
            &self.kid,
        )?;
        let resp = self.rest.submit_request(client_id, &jws).await?;
        if resp.status != "complete" {
            bail!("Login finish failed: {:?}", resp);
        }

        Ok((login_finish.session_key, session_id))
    }

    /// Generate an HSM key. Returns the hsm_kid.
    pub async fn hsm_generate_key(
        &self,
        session_key: &[u8],
        session_id: &str,
        client_id: &str,
    ) -> Result<String> {
        let payload = serde_json::to_value(CreateKeyServiceData { curve: Curve::P256 })?;
        let jws = build_session_request_jws(
            OperationId::HsmGenerateKey,
            &payload,
            session_id,
            3,
            session_key,
            &self.device_private_key,
            &self.kid,
        )?;
        let resp = self.rest.submit_request(client_id, &jws).await?;
        if resp.status != "complete" {
            bail!("HSM generate key failed: {:?}", resp);
        }
        let result_jws = resp
            .result
            .context("No result in HSM generate key response")?;

        let session_resp = unwrap_session_response(&result_jws, session_key)?;
        if session_resp.status != "OK" {
            bail!(
                "HSM generate key response error: status={}",
                session_resp.status
            );
        }
        let data_str = session_resp
            .data
            .context("No data in HSM generate key response")?;
        let hsm_resp: CreateKeyServiceDataResponse =
            serde_json::from_str(&data_str).context("Failed to parse HSM generate key response")?;
        let hsm_kid = hsm_resp.public_key.kid.clone();

        Ok(hsm_kid)
    }

    /// Sign with an HSM key. Returns the result JWS from the server.
    pub async fn hsm_sign(
        &self,
        session_key: &[u8],
        session_id: &str,
        client_id: &str,
        hsm_kid: &str,
        message: &[u8],
    ) -> Result<String> {
        let payload = serde_json::to_value(SignRequest {
            hsm_kid: hsm_kid.to_string(),
            message: MessageVector::new(message.to_vec()),
        })?;
        let jws = build_session_request_jws(
            OperationId::HsmSign,
            &payload,
            session_id,
            3,
            session_key,
            &self.device_private_key,
            &self.kid,
        )?;
        let resp = self.rest.submit_request(client_id, &jws).await?;
        if resp.status != "complete" {
            bail!("HSM sign failed: {:?}", resp);
        }
        let result_jws = resp.result.context("No result in HSM sign response")?;

        // Verify the inner response status
        let session_resp = unwrap_session_response(&result_jws, session_key)?;
        if session_resp.status != "OK" {
            bail!("HSM sign response error: status={}", session_resp.status);
        }

        Ok(result_jws)
    }
}

/// Load a server public key from a PEM file and return it as a josekit Jwk.
pub fn load_server_public_key_pem(pem_path: &str) -> Result<Jwk> {
    let pem_content = std::fs::read_to_string(pem_path)
        .with_context(|| format!("Failed to read PEM file: {}", pem_path))?;

    Jwk::from_bytes(pem_to_jwk_bytes(&pem_content).context("Failed to convert PEM to JWK")?)
        .context("Failed to parse JWK")
}

/// Convert an EC P-256 public key PEM to a JWK JSON bytes.
fn pem_to_jwk_bytes(pem_str: &str) -> Result<Vec<u8>> {
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    use p256::PublicKey;

    let public_key = PublicKey::from_sec1_bytes(&pem_to_sec1_bytes(pem_str)?).or_else(|_| {
        // Try parsing as SPKI PEM
        let pk: PublicKey = pem_str
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse PEM as P-256 public key: {:?}", e))?;
        Ok::<_, anyhow::Error>(pk)
    })?;

    let point = public_key.to_encoded_point(false);
    let x = URL_SAFE_NO_PAD.encode(point.x().unwrap());
    let y = URL_SAFE_NO_PAD.encode(point.y().unwrap());

    Ok(serde_json::to_vec(&serde_json::json!({
        "kty": "EC",
        "crv": "P-256",
        "x": x,
        "y": y,
    }))?)
}

/// Try to extract SEC1 uncompressed bytes from a PEM string.
fn pem_to_sec1_bytes(pem_str: &str) -> Result<Vec<u8>> {
    let pem_data = pem::parse(pem_str).context("Failed to parse PEM")?;
    let der = pem_data.contents();

    // If it's a SubjectPublicKeyInfo (SPKI), extract the bitstring contents
    // SPKI for P-256: SEQUENCE { SEQUENCE { OID ecPublicKey, OID prime256v1 }, BITSTRING { 04||x||y } }
    // The uncompressed point starts at a known offset in the DER
    if der.len() == 91 {
        // Standard SPKI for P-256: 91 bytes, point starts at byte 26
        // (after the outer SEQUENCE + inner SEQUENCE + BIT STRING overhead)
        Ok(der[26..].to_vec())
    } else if der.len() == 65 && der[0] == 0x04 {
        // Raw uncompressed point
        Ok(der.to_vec())
    } else {
        // Fallback: try to find 0x04 marker for uncompressed point
        // The BIT STRING in SPKI has a leading 0x00 (unused bits) before the point
        if let Some(pos) = der.iter().position(|&b| b == 0x04) {
            if der.len() - pos == 65 {
                return Ok(der[pos..].to_vec());
            }
        }
        bail!(
            "Unsupported PEM format (DER length: {}). Expected SPKI or raw EC point.",
            der.len()
        );
    }
}

/// Build a josekit Jwk from device key parameters (for JWS signing and JWE decryption).
pub fn build_device_jwk(x: &str, y: &str, d: &str, kid: &str) -> Result<Jwk> {
    let jwk_json = serde_json::json!({
        "kty": "EC",
        "crv": "P-256",
        "x": x,
        "y": y,
        "d": d,
        "kid": kid,
    });
    Jwk::from_bytes(serde_json::to_vec(&jwk_json)?).context("Failed to build device JWK")
}
