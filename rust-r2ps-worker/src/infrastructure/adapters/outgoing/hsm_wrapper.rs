use crate::application::hsm_spi_port::HsmSpiPort;
use crate::domain::{Curve, EcPublicJwk, HsmKey};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cryptoki::context::{CInitializeArgs, Pkcs11};
use cryptoki::error::Error;
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, KeyType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use der::Decode;
use der::asn1::OctetStringRef;
use digest::Digest;
use p256::ecdsa::VerifyingKey;
use std::env;
use std::sync::Arc;
use tracing::{info, warn};

pub struct HsmWrapper {
    pkcs11: Arc<Pkcs11>,
    slot: Slot,
    so_pin: Option<AuthPin>,
    user_pin: Option<AuthPin>,
    wrap_key_alias: Vec<u8>,
}

#[derive(Debug)]
pub struct Pkcs11Config {
    pub lib_path: String,
    pub slot_index: usize,
    pub so_pin: Option<String>,
    pub user_pin: Option<String>,
    pub wrap_key_alias: String,
}

impl Pkcs11Config {
    pub fn new_from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let lib_path = env::var("PKCS11LIB").map_err(|_| "PKCS11LIB env var not set")?;
        let slot_index_str = env::var("PKCS11_SLOT").map_err(|_| "PKCS11SLOT env var not set")?;
        let slot_index: usize = slot_index_str.parse()?;
        let so_pin: Option<String> = env::var("SO_PIN").ok();
        let user_pin = env::var("USER_PIN").ok();
        let wrap_key_alias = env::var("WRAPPER_KEY_ALIAS")
            .ok()
            .or_else(|| Some("aes-wrapping-key".to_string()))
            .unwrap();

        Ok(Pkcs11Config {
            lib_path,
            slot_index,
            so_pin,
            user_pin,
            wrap_key_alias,
        })
    }
}

impl HsmWrapper {
    pub fn new(config: Pkcs11Config) -> Result<Self, Box<dyn std::error::Error>> {
        info!("Creating HSM wrapper with config {:?}", config);
        // 1. Initialize the PKCS#11 context
        let pkcs11 = Arc::new(Pkcs11::new(config.lib_path)?);

        pkcs11.initialize(CInitializeArgs::OsThreads)?;
        // 2. Find the target slot ID
        let slots = pkcs11.get_slots_with_token()?;
        if slots.len() <= config.slot_index {
            return Err(format!("Slot index {} not found.", config.slot_index).into());
        }
        //let slot_id = slots[config.slot_index].id();
        let slot = slots[config.slot_index];

        info!("Slot index {} is {}.", config.slot_index, slot);
        // initialize a test token
        let so_pin = config.so_pin.map(AuthPin::new);
        let user_pin = config.user_pin.map(AuthPin::new);

        let wrap_key_alias = config.wrap_key_alias.as_bytes().to_vec();

        let session = pkcs11.open_rw_session(slot)?;

        session.login(UserType::User, Some(&user_pin.clone().unwrap()))?;

        let result = HsmWrapper {
            pkcs11,
            slot,
            wrap_key_alias,
            so_pin,
            user_pin,
        };

        // verify that wrapping key is initialized - otherwise create one TODO other method...
        result.aes_wrapping_key(&session)?;

        Ok(result)
    }

    fn init_token_and_pin(&self) -> Result<(), Box<dyn std::error::Error>> {
        let session = self.pkcs11.open_rw_session(self.slot)?;
        session.login(UserType::So, self.so_pin.as_ref())?;
        if let Some(ref user_pin) = self.user_pin {
            session.init_pin(user_pin)?;
        }
        Ok(())
    }

    pub fn ec_key_templates(&self, label: &str, curve: &Curve) -> (Vec<Attribute>, Vec<Attribute>) {
        let ec_params = match *curve {
            Curve::P256 => vec![0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07],
            Curve::P384 => vec![0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x22],
            Curve::P521 => vec![0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x23],
        };

        (
            vec![
                Attribute::Private(true),
                Attribute::Token(false),
                Attribute::Sensitive(true),
                Attribute::Extractable(true),
                Attribute::Sign(true),
                Attribute::Label(format!("{}-private", label).into()),
            ],
            vec![
                Attribute::Private(false),
                Attribute::Token(false),
                Attribute::EcParams(ec_params),
                Attribute::Verify(true),
                Attribute::Label(format!("{}-public", label).into()),
            ],
        )
    }

    pub fn aes_wrapping_key(&self, session: &Session) -> Result<ObjectHandle, Error> {
        let aes_template = vec![
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::ValueLen(32.into()), // 256-bit
            Attribute::Encrypt(true),
            Attribute::Decrypt(true),
            Attribute::Wrap(true),
            Attribute::Unwrap(true),
            Attribute::Label(self.wrap_key_alias.clone()),
        ];

        match session.find_objects(&aes_template)?.first() {
            Some(aes_key) => Ok(aes_key.to_owned()),
            None => {
                warn!("No wrapping key found... generate new aes wrapping key");
                session.generate_key(&Mechanism::AesKeyGen, &aes_template)
            }
        }
    }

    pub fn wrap_private_key(
        &self,
        session: &Session,
        ec_private_key: ObjectHandle,
    ) -> Result<Vec<u8>, Error> {
        let mechanism = Mechanism::AesKeyWrapPad;
        let wrapping_key = self.aes_wrapping_key(session)?;

        session.wrap_key(&mechanism, wrapping_key, ec_private_key)
    }

    pub fn unwrap_private_key(
        &self,
        session: &Session,
        wrapped_private_key: Vec<u8>,
    ) -> Result<ObjectHandle, Error> {
        let mechanism = Mechanism::AesKeyWrapPad;
        let wrap_key = self.aes_wrapping_key(session)?;

        session.unwrap_key(
            &mechanism,
            wrap_key,
            &wrapped_private_key,
            &[
                Attribute::Class(ObjectClass::PRIVATE_KEY),
                Attribute::KeyType(KeyType::EC),
                Attribute::Private(true),
                Attribute::Token(false),
                Attribute::Sensitive(true),
                Attribute::Extractable(true),
                Attribute::Sign(true),
            ],
        )
    }

    pub fn create_ec_public_key_jwk(
        &self,
        session: &Session,
        public_key: ObjectHandle,
        curve: &Curve,
    ) -> Result<EcPublicJwk, Box<dyn std::error::Error>> {
        let attrs = session.get_attributes(public_key, &[AttributeType::EcPoint])?;

        match attrs.first() {
            Some(Attribute::EcPoint(point)) => Self::ec_point_to_jwk(curve, point),
            _ => Err("EC point not found".into()),
        }
    }

    fn ec_point_to_jwk(
        curve: &Curve,
        point: &[u8],
    ) -> Result<EcPublicJwk, Box<dyn std::error::Error>> {
        let octet_string = OctetStringRef::from_der(point).map_err(|e| e.to_string())?;

        let verifying_key =
            VerifyingKey::from_sec1_bytes(octet_string.as_bytes()).map_err(|e| e.to_string())?;

        let ec_point = verifying_key.to_encoded_point(false);
        let x = ec_point.x().ok_or("X coordinate not found")?;
        let y = ec_point.y().ok_or("Y coordinate not found")?;

        let x_b64 = URL_SAFE_NO_PAD.encode(x);
        let y_b64 = URL_SAFE_NO_PAD.encode(y);

        let kid = Self::generate_kid(curve, &x_b64, &y_b64);

        Ok(EcPublicJwk {
            crv: curve.to_string(),
            kty: "EC".to_string(),
            x: x_b64,
            y: y_b64,
            kid,
        })
    }

    fn generate_kid(curve: &Curve, x_b64: &String, y_b64: &String) -> String {
        let thumbprint = format!(
            r#"{{"crv":"{}","kty":"EC","x":"{}","y":"{}"}}"#,
            curve, x_b64, y_b64
        );

        let mut hasher = sha2::Sha256::new();
        hasher.update(thumbprint.as_bytes());
        URL_SAFE_NO_PAD.encode(hasher.finalize())
    }
}

impl HsmSpiPort for HsmWrapper {
    fn generate_key(
        &self,
        label: &str,
        curve: &Curve,
    ) -> Result<HsmKey, Box<dyn std::error::Error>> {
        let session = self.pkcs11.open_ro_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;

        let (private_key_template, public_key_template) = self.ec_key_templates(label, curve);
        let (ec_public_key, ec_private_key) = session.generate_key_pair(
            &Mechanism::EccKeyPairGen,
            &public_key_template,
            &private_key_template,
        )?;

        let wrapped_private_key = self.wrap_private_key(&session, ec_private_key)?;
        let public_key_jwk = self.create_ec_public_key_jwk(&session, ec_public_key, curve)?;

        session.close();
        println!(
            "Successfully generated EC key pair with label: {} {}",
            ec_public_key,
            wrapped_private_key.len()
        );

        Ok(HsmKey {
            wrapped_private_key,
            public_key_jwk,
            curve_name: curve.clone(),
            creation_time: chrono::Utc::now(),
        })
    }

    fn sign(&self, wrapped_key: &[u8], sign_payload: &[u8]) -> Result<Vec<u8>, Error> {
        let session = self.pkcs11.open_rw_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;
        let private_key = self.unwrap_private_key(&session, wrapped_key.to_vec())?;
        let signature = session.sign(&Mechanism::Ecdsa, private_key, sign_payload)?;
        session.close();
        Ok(signature)
    }
}

impl Drop for HsmWrapper {
    fn drop(&mut self) {
        info!("HsmWrapper dropped. PKCS#11 context will finalize when all Arcs are dropped.");
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::Curve;
    use crate::infrastructure::hsm_wrapper::HsmWrapper;
    use p256::ecdsa::SigningKey;
    use rand::rngs::OsRng;

    // Verify that the EcPublicJwk generated is a valid JWK according to JOSE.
    #[test]
    pub fn test_ec_point_to_jwk() -> Result<(), Box<dyn std::error::Error>> {
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let encoded_point = verifying_key.to_encoded_point(false);
        let sec1_bytes = encoded_point.as_bytes();
        let mut der_encoded = vec![0x04, sec1_bytes.len() as u8];
        der_encoded.extend_from_slice(sec1_bytes);

        let jwk = HsmWrapper::ec_point_to_jwk(&Curve::P256, &der_encoded)?;

        let json_output = serde_json::to_string(&jwk)?;
        let jose_jwk = josekit::jwk::Jwk::from_bytes(json_output.as_bytes())?;

        assert_eq!(jose_jwk.key_type(), "EC");
        assert_eq!(
            jose_jwk.parameter("crv").unwrap().as_str().unwrap(),
            "P-256"
        );
        assert_eq!(jose_jwk.parameter("x").unwrap().as_str().unwrap(), &jwk.x);
        assert_eq!(jose_jwk.parameter("y").unwrap().as_str().unwrap(), &jwk.y);
        Ok(())
    }
}
