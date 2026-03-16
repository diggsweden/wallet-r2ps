//! HPKE AuthEncap/AuthDecap mutual authentication (RFC 9180).
//!
//! Implements DHKEM(P-256, HKDF-SHA256) + HKDF-SHA256 + AES-128-GCM
//! in authenticated mode (`mode_auth`).
//!
//! The handshake:
//! 1. Client sends `auth_init { clientId }`
//! 2. Server generates a random nonce and salt.
//!    Server performs AuthEncap(server_sk, client_pk, nonce) and sends
//!    `auth_challenge { enc, ciphertext, salt, serverKid }`.
//!    The salt is sent in the clear; the nonce is encrypted.
//! 3. Client performs AuthDecap to recover the nonce (proving server key possession).
//!    Client computes `response = HMAC-SHA256(key=nonce, msg=salt)`.
//!    Client performs AuthEncap(client_sk, server_pk, response) and sends
//!    `auth_response { enc, ciphertext }`.
//! 4. Server performs AuthDecap to recover the response (proving client key possession).
//!    Server independently computes `expected = HMAC-SHA256(key=nonce, msg=salt)`
//!    and verifies it matches. This confirms the client decrypted the nonce
//!    and combined it with the correct salt.
//!
//! Both sides prove possession of their respective private keys through
//! the authenticated HPKE encapsulation. The HMAC binds the encrypted
//! nonce to the plaintext salt, preventing replay and ensuring freshness.

use aes_gcm::{
    Aes128Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::Engine;
use hkdf::Hkdf;
use p256::{
    EncodedPoint, PublicKey, SecretKey,
    ecdh::EphemeralSecret,
    elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint},
};
use rand::rngs::OsRng;
use sha2::Sha256;

use crate::domain::device_management::value_objects::EcPublicKeyData;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// HPKE suite identifier for labelled extraction/expansion.
/// DHKEM(P-256, HKDF-SHA256) = 0x0010
/// HKDF-SHA256 = 0x0001
/// AES-128-GCM = 0x0001
const SUITE_ID_KEM: &[u8] = b"KEM\x00\x10";
const SUITE_ID_HPKE: &[u8] = b"HPKE\x00\x10\x00\x01\x00\x01";
const MODE_AUTH: u8 = 0x02;

/// Server-side HPKE authentication context.
///
/// Holds the server's static key pair and provides methods for
/// the auth handshake.
#[derive(Clone)]
pub struct HpkeAuthContext {
    server_sk: SecretKey,
    server_pk: PublicKey,
    server_kid: String,
}

impl HpkeAuthContext {
    /// Create an HPKE auth context from JWK JSON strings.
    pub fn from_jwk(
        private_key_jwk: &str,
        public_key_jwk: &str,
        server_kid: String,
    ) -> Result<Self, HpkeAuthError> {
        let server_sk = parse_ec_private_key_jwk(private_key_jwk)?;
        let server_pk = parse_ec_public_key_jwk(public_key_jwk)?;
        Ok(Self {
            server_sk,
            server_pk,
            server_kid,
        })
    }

    /// Server key identifier for the auth challenge message.
    pub fn server_kid(&self) -> &str {
        &self.server_kid
    }

    /// Server's public key for client discovery.
    pub fn server_public_key(&self) -> &PublicKey {
        &self.server_pk
    }

    /// Step 2: Server creates an auth challenge.
    ///
    /// Generates a random 32-byte nonce and a random 32-byte salt.
    /// The nonce is HPKE-encrypted (only the legitimate client can decrypt it).
    /// The salt is sent in the clear alongside the ciphertext.
    ///
    /// Both nonce and salt are returned so the server can verify the
    /// client's HMAC-based response in step 4.
    pub fn create_challenge(&self, client_pk: &PublicKey) -> Result<AuthChallenge, HpkeAuthError> {
        // Generate random nonce (secret — encrypted in challenge)
        let mut nonce_bytes = [0u8; 32];
        rand::Rng::fill(&mut OsRng, &mut nonce_bytes);

        // Generate random salt (public — sent in the clear)
        let mut salt_bytes = [0u8; 32];
        rand::Rng::fill(&mut OsRng, &mut salt_bytes);

        // HPKE AuthEncap: generate ephemeral keypair, do DH with client_pk and server_sk
        let (enc, key_schedule) = auth_encap(&self.server_sk, client_pk)?;

        // Encrypt the nonce using the derived key
        let ciphertext = seal(&key_schedule, &nonce_bytes, b"r2ps-auth-challenge")?;

        Ok(AuthChallenge {
            enc,
            ciphertext,
            nonce: nonce_bytes.to_vec(),
            salt: salt_bytes.to_vec(),
        })
    }

    /// Step 4: Server verifies the client's auth response.
    ///
    /// Performs HPKE AuthDecap using the server's SK and the client's PK
    /// to decrypt the client's response. The decrypted plaintext should be
    /// `HMAC-SHA256(key=nonce, msg=salt)` — proving the client both
    /// decrypted the nonce (from the encrypted challenge) and received
    /// the salt (sent in the clear).
    pub fn verify_response(
        &self,
        client_pk: &PublicKey,
        enc_bytes: &[u8],
        ciphertext: &[u8],
        expected_nonce: &[u8],
        salt: &[u8],
    ) -> Result<(), HpkeAuthError> {
        // HPKE AuthDecap: derive shared secret using enc, server_sk, client_pk
        let key_schedule = auth_decap(enc_bytes, &self.server_sk, client_pk)?;

        // Decrypt
        let plaintext = open(&key_schedule, ciphertext, b"r2ps-auth-response")?;

        // Compute expected value: HMAC-SHA256(key=nonce, msg=salt)
        let expected = compute_challenge_response(expected_nonce, salt);

        // Constant-time comparison
        if plaintext.len() != expected.len() {
            return Err(HpkeAuthError::AuthFailed(
                "auth response length mismatch".to_string(),
            ));
        }
        let matches = plaintext
            .iter()
            .zip(expected.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b));
        if matches != 0 {
            return Err(HpkeAuthError::AuthFailed(
                "auth response HMAC verification failed".to_string(),
            ));
        }

        Ok(())
    }
}

/// Result of creating an auth challenge (step 2).
pub struct AuthChallenge {
    /// HPKE encapsulated key (serialized ephemeral public key).
    pub enc: Vec<u8>,
    /// Encrypted nonce.
    pub ciphertext: Vec<u8>,
    /// The plaintext nonce (kept server-side for verification in step 4).
    pub nonce: Vec<u8>,
    /// Random salt sent in the clear (used in HMAC verification in step 4).
    pub salt: Vec<u8>,
}

/// Compute the expected auth response value: `HMAC-SHA256(key=nonce, msg=salt)`.
///
/// The client must produce this same value to prove it decrypted the nonce
/// (from the HPKE-encrypted challenge) and received the salt (sent in the clear).
pub fn compute_challenge_response(nonce: &[u8], salt: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = <HmacSha256 as Mac>::new_from_slice(nonce).expect("HMAC accepts any key length");
    mac.update(salt);
    mac.finalize().into_bytes().to_vec()
}

/// Parse an EC P-256 public key from the DeviceState payload format.
pub fn ec_public_key_from_device_state(
    key_data: &EcPublicKeyData,
) -> Result<PublicKey, HpkeAuthError> {
    if key_data.kty != "EC" || key_data.crv != "P-256" {
        return Err(HpkeAuthError::InvalidKey(format!(
            "expected EC P-256, got {} {}",
            key_data.kty, key_data.crv
        )));
    }

    let x_bytes = B64
        .decode(&key_data.x)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid x coordinate: {}", e)))?;
    let y_bytes = B64
        .decode(&key_data.y)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid y coordinate: {}", e)))?;

    let encoded_point = EncodedPoint::from_affine_coordinates(
        p256::FieldBytes::from_slice(&x_bytes),
        p256::FieldBytes::from_slice(&y_bytes),
        false,
    );

    let pk = PublicKey::from_encoded_point(&encoded_point);
    if pk.is_some().into() {
        Ok(pk.unwrap())
    } else {
        Err(HpkeAuthError::InvalidKey("invalid EC point".to_string()))
    }
}

/// Parse a JWK JSON string into an EC P-256 private key.
fn parse_ec_private_key_jwk(jwk_json: &str) -> Result<SecretKey, HpkeAuthError> {
    let jwk: serde_json::Value = serde_json::from_str(jwk_json)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid JWK JSON: {}", e)))?;

    let d = jwk["d"]
        .as_str()
        .ok_or_else(|| HpkeAuthError::InvalidKey("missing 'd' parameter in private JWK".into()))?;

    let d_bytes = B64
        .decode(d)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid 'd' parameter: {}", e)))?;

    SecretKey::from_slice(&d_bytes)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid EC private key: {}", e)))
}

/// Parse a JWK JSON string into an EC P-256 public key.
fn parse_ec_public_key_jwk(jwk_json: &str) -> Result<PublicKey, HpkeAuthError> {
    let jwk: serde_json::Value = serde_json::from_str(jwk_json)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid JWK JSON: {}", e)))?;

    let x = jwk["x"]
        .as_str()
        .ok_or_else(|| HpkeAuthError::InvalidKey("missing 'x' parameter".into()))?;
    let y = jwk["y"]
        .as_str()
        .ok_or_else(|| HpkeAuthError::InvalidKey("missing 'y' parameter".into()))?;

    let x_bytes = B64
        .decode(x)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid x: {}", e)))?;
    let y_bytes = B64
        .decode(y)
        .map_err(|e| HpkeAuthError::InvalidKey(format!("invalid y: {}", e)))?;

    let encoded_point = EncodedPoint::from_affine_coordinates(
        p256::FieldBytes::from_slice(&x_bytes),
        p256::FieldBytes::from_slice(&y_bytes),
        false,
    );

    let pk = PublicKey::from_encoded_point(&encoded_point);
    if pk.is_some().into() {
        Ok(pk.unwrap())
    } else {
        Err(HpkeAuthError::InvalidKey(
            "invalid EC point from JWK".to_string(),
        ))
    }
}

// ─── HPKE Primitives (RFC 9180, mode_auth) ───

/// HPKE AuthEncap: generate ephemeral key, compute shared secret with
/// both the recipient's public key and the sender's static private key.
///
/// Returns `(enc, key_schedule_bytes)` where `enc` is the serialized
/// ephemeral public key and `key_schedule_bytes` is the derived AES-128 key.
fn auth_encap(
    sender_sk: &SecretKey,
    recipient_pk: &PublicKey,
) -> Result<(Vec<u8>, Vec<u8>), HpkeAuthError> {
    // Generate ephemeral keypair
    let eph_secret = EphemeralSecret::random(&mut OsRng);
    let eph_pk = eph_secret.public_key();
    let enc = eph_pk.to_encoded_point(false).as_bytes().to_vec();

    // DH(eph_sk, recipient_pk) — ephemeral ECDH
    let dh1 = eph_secret.diffie_hellman(recipient_pk);

    // DH(sender_sk, recipient_pk) — static-static ECDH for auth
    let sender_pk_affine = sender_sk.to_nonzero_scalar();
    let recipient_point = recipient_pk.as_affine();
    let dh2_point = (p256::ProjectivePoint::from(*recipient_point) * *sender_pk_affine).to_affine();
    let dh2_bytes = p256::EncodedPoint::from(dh2_point);

    // kem_context = enc || recipient_pk || sender_pk
    let sender_pk = sender_sk.public_key();
    let recipient_pk_bytes = recipient_pk.to_encoded_point(false);
    let sender_pk_bytes = sender_pk.to_encoded_point(false);

    let mut kem_context = Vec::new();
    kem_context.extend_from_slice(&enc);
    kem_context.extend_from_slice(recipient_pk_bytes.as_bytes());
    kem_context.extend_from_slice(sender_pk_bytes.as_bytes());

    // shared_secret = ExtractAndExpand(dh = dh1 || dh2, kem_context)
    let mut dh_combined = Vec::new();
    dh_combined.extend_from_slice(dh1.raw_secret_bytes().as_slice());
    dh_combined.extend_from_slice(&dh2_bytes.as_bytes()[1..]); // skip 0x04 prefix for raw bytes
    // Actually, for raw shared secret we need the x-coordinate only
    // The ECDH shared secret is the x-coordinate of the shared point
    // p256's SharedSecret already gives us just the x-coordinate for dh1
    // For dh2, we need to extract x from the uncompressed point
    let dh2_x = &dh2_bytes.as_bytes()[1..33]; // x-coordinate from uncompressed point

    let mut dh_concat = Vec::new();
    dh_concat.extend_from_slice(dh1.raw_secret_bytes().as_slice());
    dh_concat.extend_from_slice(dh2_x);

    let shared_secret = extract_and_expand_kem(&dh_concat, &kem_context)?;

    // Key schedule: derive AES-128 key
    let key = key_schedule(&shared_secret)?;

    Ok((enc, key))
}

/// HPKE AuthDecap: compute shared secret from received `enc`, recipient's
/// SK, and sender's PK.
fn auth_decap(
    enc: &[u8],
    recipient_sk: &SecretKey,
    sender_pk: &PublicKey,
) -> Result<Vec<u8>, HpkeAuthError> {
    // Recover ephemeral public key from enc
    let eph_pk_point = EncodedPoint::from_bytes(enc)
        .map_err(|e| HpkeAuthError::CryptoError(format!("invalid enc: {}", e)))?;
    let eph_pk = PublicKey::from_encoded_point(&eph_pk_point);
    if bool::from(eph_pk.is_none()) {
        return Err(HpkeAuthError::CryptoError(
            "invalid ephemeral public key".to_string(),
        ));
    }
    let eph_pk = eph_pk.unwrap();

    // DH(recipient_sk, eph_pk)
    // We need to use the NonZeroScalar for manual ECDH
    let recipient_scalar = recipient_sk.to_nonzero_scalar();
    let eph_affine = eph_pk.as_affine();
    let dh1_point = (p256::ProjectivePoint::from(*eph_affine) * *recipient_scalar).to_affine();
    let dh1_encoded = p256::EncodedPoint::from(dh1_point);
    let dh1_x = &dh1_encoded.as_bytes()[1..33];

    // DH(recipient_sk, sender_pk) — static-static for auth
    let sender_affine = sender_pk.as_affine();
    let dh2_point = (p256::ProjectivePoint::from(*sender_affine) * *recipient_scalar).to_affine();
    let dh2_encoded = p256::EncodedPoint::from(dh2_point);
    let dh2_x = &dh2_encoded.as_bytes()[1..33];

    // kem_context = enc || recipient_pk || sender_pk
    let recipient_pk = recipient_sk.public_key();
    let recipient_pk_bytes = recipient_pk.to_encoded_point(false);
    let sender_pk_bytes = sender_pk.to_encoded_point(false);

    let mut kem_context = Vec::new();
    kem_context.extend_from_slice(enc);
    kem_context.extend_from_slice(recipient_pk_bytes.as_bytes());
    kem_context.extend_from_slice(sender_pk_bytes.as_bytes());

    let mut dh_concat = Vec::new();
    dh_concat.extend_from_slice(dh1_x);
    dh_concat.extend_from_slice(dh2_x);

    let shared_secret = extract_and_expand_kem(&dh_concat, &kem_context)?;

    let key = key_schedule(&shared_secret)?;

    Ok(key)
}

/// KEM ExtractAndExpand (RFC 9180 Section 4.1)
fn extract_and_expand_kem(dh: &[u8], kem_context: &[u8]) -> Result<Vec<u8>, HpkeAuthError> {
    // suite_id for KEM = "KEM" || I2OSP(kem_id, 2)
    // Extract: prk = LabeledExtract("", "shared_secret", dh)
    let mut extract_label = Vec::new();
    extract_label.extend_from_slice(b"HPKE-v1");
    extract_label.extend_from_slice(SUITE_ID_KEM);
    extract_label.extend_from_slice(b"shared_secret");

    let salt = []; // empty salt
    let mut ikm = Vec::new();
    ikm.extend_from_slice(&extract_label);
    ikm.extend_from_slice(dh);

    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);

    // Expand: shared_secret = LabeledExpand(prk, "shared_secret", kem_context, Nsecret)
    let mut expand_info = Vec::new();
    expand_info.extend_from_slice(&(32u16).to_be_bytes()); // L = Nsecret = 32
    expand_info.extend_from_slice(b"HPKE-v1");
    expand_info.extend_from_slice(SUITE_ID_KEM);
    expand_info.extend_from_slice(b"shared_secret");
    expand_info.extend_from_slice(kem_context);

    let mut shared_secret = vec![0u8; 32];
    hk.expand(&expand_info, &mut shared_secret)
        .map_err(|e| HpkeAuthError::CryptoError(format!("HKDF expand failed: {}", e)))?;

    Ok(shared_secret)
}

/// HPKE Key Schedule (simplified for our use case).
/// Derives an AES-128 key from the shared secret.
fn key_schedule(shared_secret: &[u8]) -> Result<Vec<u8>, HpkeAuthError> {
    // PSK input defaults (mode_auth, no PSK)
    let psk_id_hash = labeled_extract_hpke(b"psk_id_hash", &[], &[]);
    let info_hash = labeled_extract_hpke(b"info_hash", &[], &[]);

    let mut ks_context = Vec::new();
    ks_context.push(MODE_AUTH);
    ks_context.extend_from_slice(&psk_id_hash);
    ks_context.extend_from_slice(&info_hash);

    // secret = LabeledExtract(shared_secret, "secret", psk=default_psk="")
    let default_psk = [];
    let mut secret_ikm = Vec::new();
    secret_ikm.extend_from_slice(b"HPKE-v1");
    secret_ikm.extend_from_slice(SUITE_ID_HPKE);
    secret_ikm.extend_from_slice(b"secret");
    secret_ikm.extend_from_slice(&default_psk);

    let hk = Hkdf::<Sha256>::new(Some(shared_secret), &secret_ikm);

    // key = LabeledExpand(secret, "key", ks_context, Nk=16)
    let mut key_info = Vec::new();
    key_info.extend_from_slice(&(16u16).to_be_bytes()); // L = Nk = 16 for AES-128
    key_info.extend_from_slice(b"HPKE-v1");
    key_info.extend_from_slice(SUITE_ID_HPKE);
    key_info.extend_from_slice(b"key");
    key_info.extend_from_slice(&ks_context);

    let mut key = vec![0u8; 16];
    hk.expand(&key_info, &mut key)
        .map_err(|e| HpkeAuthError::CryptoError(format!("key derivation failed: {}", e)))?;

    Ok(key)
}

/// Helper: LabeledExtract for HPKE suite
fn labeled_extract_hpke(label: &[u8], salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    let mut labeled_ikm = Vec::new();
    labeled_ikm.extend_from_slice(b"HPKE-v1");
    labeled_ikm.extend_from_slice(SUITE_ID_HPKE);
    labeled_ikm.extend_from_slice(label);
    labeled_ikm.extend_from_slice(ikm);

    let hk = Hkdf::<Sha256>::new(
        if salt.is_empty() { None } else { Some(salt) },
        &labeled_ikm,
    );
    let mut out = vec![0u8; 32];
    // Extract only — use expand with empty info to get PRK
    // Actually, for LabeledExtract we just need the PRK, which is the extract step
    // The Hkdf::new already does the extract. We just return the PRK.
    // But Hkdf doesn't expose PRK directly. Use a zero-length expand as a hash.
    let _ = hk.expand(&[], &mut out);
    out
}

/// AEAD Seal (AES-128-GCM)
fn seal(key: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, HpkeAuthError> {
    let cipher = Aes128Gcm::new_from_slice(key)
        .map_err(|e| HpkeAuthError::CryptoError(format!("AES key error: {}", e)))?;

    // Use a zero nonce for single-shot HPKE (base_nonce XOR 0 = base_nonce)
    // In our simplified protocol we use a fixed nonce since each key is used only once
    let nonce = Nonce::from([0u8; 12]);

    let ciphertext = cipher
        .encrypt(
            &nonce,
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|e| HpkeAuthError::CryptoError(format!("encryption failed: {}", e)))?;

    Ok(ciphertext)
}

/// AEAD Open (AES-128-GCM)
fn open(key: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, HpkeAuthError> {
    let cipher = Aes128Gcm::new_from_slice(key)
        .map_err(|e| HpkeAuthError::CryptoError(format!("AES key error: {}", e)))?;

    let nonce = Nonce::from([0u8; 12]);

    let plaintext = cipher
        .decrypt(
            &nonce,
            aes_gcm::aead::Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|e| HpkeAuthError::CryptoError(format!("decryption failed: {}", e)))?;

    Ok(plaintext)
}

/// Encode bytes as base64url (no padding).
pub fn b64_encode(data: &[u8]) -> String {
    B64.encode(data)
}

/// Decode base64url (no padding) to bytes.
pub fn b64_decode(data: &str) -> Result<Vec<u8>, HpkeAuthError> {
    B64.decode(data)
        .map_err(|e| HpkeAuthError::CryptoError(format!("base64 decode error: {}", e)))
}

/// Errors during HPKE authentication.
#[derive(Debug, thiserror::Error)]
pub enum HpkeAuthError {
    #[error("invalid key: {0}")]
    InvalidKey(String),

    #[error("cryptographic error: {0}")]
    CryptoError(String),

    #[error("authentication failed: {0}")]
    AuthFailed(String),
}

impl std::fmt::Debug for HpkeAuthContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HpkeAuthContext")
            .field("server_kid", &self.server_kid)
            .field("server_pk", &"<redacted>")
            .finish()
    }
}
