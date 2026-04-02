use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use digest::Digest;
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use sha2::Sha256;

use crate::domain::EcPublicJwk;

/// Build an `EcPublicJwk` from a P-256 secret key, including a JWK thumbprint KID.
pub fn ec_public_key_from_secret(secret_key: &SecretKey) -> EcPublicJwk {
    let point = secret_key.public_key().as_affine().to_encoded_point(false);
    let x = BASE64_URL_SAFE_NO_PAD.encode(point.x().expect("x coordinate"));
    let y = BASE64_URL_SAFE_NO_PAD.encode(point.y().expect("y coordinate"));
    let kid = ec_kid_from_x_y(&x, &y);
    EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x,
        y,
        kid,
    }
}

/// JWK thumbprint KID for a P-256 secret key (SHA-256 of canonical JWK, base64url).
pub fn ec_kid_from_secret(key: &SecretKey) -> String {
    let point = key.public_key().as_affine().to_encoded_point(false);
    let x = BASE64_URL_SAFE_NO_PAD.encode(point.x().expect("x coordinate"));
    let y = BASE64_URL_SAFE_NO_PAD.encode(point.y().expect("y coordinate"));
    ec_kid_from_x_y(&x, &y)
}

fn ec_kid_from_x_y(x: &str, y: &str) -> String {
    let thumbprint = format!(r#"{{"crv":"P-256","kty":"EC","x":"{}","y":"{}"}}"#, x, y);
    let mut hasher = Sha256::new();
    hasher.update(thumbprint.as_bytes());
    BASE64_URL_SAFE_NO_PAD.encode(hasher.finalize())
}
