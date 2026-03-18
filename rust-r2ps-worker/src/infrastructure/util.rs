use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::collections::HashMap;
use std::str::FromStr;

use josekit::jwk::Jwk;
use p256::PublicKey;
use pem::Pem;
use spki::EncodePublicKey;
use spki::der::pem::LineEnding;

pub fn ed25519_public_key_to_jwk(public_key: &[u8]) -> Result<Jwk, Box<dyn std::error::Error>> {
    // Create JWK manually for Ed25519 (OKP key type)
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();

    // Key type: OKP (Octet Key Pair) for EdDSA keys
    params.insert(
        "kty".to_string(),
        serde_json::Value::String("OKP".to_string()),
    );

    // Curve: Ed25519
    params.insert(
        "crv".to_string(),
        serde_json::Value::String("Ed25519".to_string()),
    );

    // x: the public key (base64url encoded)
    let x = URL_SAFE_NO_PAD.encode(public_key);
    params.insert("x".to_string(), serde_json::Value::String(x));

    // Optional: key use (signature)
    params.insert(
        "use".to_string(),
        serde_json::Value::String("sig".to_string()),
    );

    // Convert to JWK
    let jwk_json = serde_json::to_string(&params)?;
    let jwk = Jwk::from_bytes(jwk_json.as_bytes())?;

    Ok(jwk)
}

pub fn ec_jwk_to_pem(jwk: &Jwk) -> Result<Pem, Box<dyn std::error::Error>> {
    let jwk_json = serde_json::to_string(jwk)?;

    // 2. Parse the JWK directly into a p256 PublicKey
    // This validates the curve (P-256) and the coordinates automatically
    let public_key = PublicKey::from_jwk_str(&jwk_json).map_err(|_| "invalid jwk format")?;

    // 3. Export to PEM (SubjectPublicKeyInfo format)
    let pem = public_key
        .to_public_key_pem(LineEnding::LF)
        .map_err(|_| "invalid public key format")?;

    Ok(Pem::from_str(&pem)?)
}
