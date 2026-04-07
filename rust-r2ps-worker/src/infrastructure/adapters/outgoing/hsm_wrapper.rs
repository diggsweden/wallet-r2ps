use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::hsm_spi_port::DerivedSecret;
use crate::domain::{Curve, EcPublicJwk, HsmKey, WrappedPrivateKey};
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
use std::sync::Arc;
use tracing::{debug, error, info};

pub struct HsmWrapper {
    pkcs11: Arc<Pkcs11>,
    slot: Slot,
    user_pin: Option<AuthPin>,
    wrap_key_alias: Vec<u8>,
}

#[derive(Debug)]
pub struct Pkcs11Config {
    pub lib_path: String,
    pub slot_token_label: String,
    pub so_pin: Option<String>,
    pub user_pin: Option<String>,
    pub wrap_key_alias: String,
}

impl HsmWrapper {
    pub fn new(config: Pkcs11Config) -> Result<Self, Box<dyn std::error::Error>> {
        debug!(
            "Creating HSM wrapper with config lib_path={} slot_token_label={} wrap_key_alias={}",
            config.lib_path, config.slot_token_label, config.wrap_key_alias
        );
        // 1. Initialize the PKCS#11 context
        let pkcs11 = Arc::new(Pkcs11::new(config.lib_path)?);

        pkcs11.initialize(CInitializeArgs::OsThreads)?;
        // 2. Find the target slot ID
        let slots = pkcs11.get_slots_with_token()?;

        let slot = slots
            .iter()
            .find(|slot| match pkcs11.get_token_info(**slot) {
                Ok(token_info) => {
                    if token_info.label().trim() == config.slot_token_label {
                        info!(
                            "Found slot with token label: {} id:{}",
                            config.slot_token_label,
                            slot.id()
                        );
                        true
                    } else {
                        false
                    }
                }
                Err(_) => false,
            })
            .unwrap_or_else(|| {
                error!("Invalid slot_token_label: {}", config.slot_token_label);
                panic!("Invalid slot_token_label");
            });

        // initialize a test token
        let user_pin = config.user_pin.map(AuthPin::new);

        let wrap_key_alias = config.wrap_key_alias.as_bytes().to_vec();

        let session = pkcs11.open_rw_session(*slot)?;

        session.login(UserType::User, Some(&user_pin.clone().unwrap()))?;

        let result = HsmWrapper {
            pkcs11,
            slot: *slot,
            wrap_key_alias,
            user_pin,
        };

        // verify that wrapping key is present — the keytool is responsible for creating it
        if !result.wrap_key_alias.is_empty() {
            result.aes_wrapping_key(&session)?;
        }

        Ok(result)
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

    pub fn exists_by_label(&self, class: ObjectClass, label: &str) -> Result<bool, Error> {
        let session = self.pkcs11.open_ro_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;
        let handles = session.find_objects(&[
            Attribute::Class(class),
            Attribute::Label(label.as_bytes().to_vec()),
        ])?;
        session.close();
        Ok(!handles.is_empty())
    }

    /// Find and destroy all objects matching class + label in a single RW session.
    pub fn destroy_objects_by_label(&self, class: ObjectClass, label: &str) -> Result<(), Error> {
        let session = self.pkcs11.open_rw_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;
        let handles = session.find_objects(&[
            Attribute::Class(class),
            Attribute::Label(label.as_bytes().to_vec()),
        ])?;
        for handle in handles {
            session.destroy_object(handle)?;
        }
        session.close();
        Ok(())
    }

    /// Create a persistent HMAC-SHA512 root key in the HSM for key derivation ceremonies.
    /// Called by `digg-hsm-keytool`, not by the service at startup.
    pub fn create_hmac_root_key(&self, label: &str) -> Result<(), Error> {
        let template = vec![
            Attribute::Class(ObjectClass::SECRET_KEY),
            Attribute::KeyType(KeyType::GENERIC_SECRET),
            Attribute::ValueLen(64.into()), // 512-bit key for HMAC-SHA512
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::Sensitive(true),
            Attribute::Extractable(false),
            Attribute::Sign(true),
            Attribute::Label(label.as_bytes().to_vec()),
        ];
        let session = self.pkcs11.open_rw_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;
        session.generate_key(&Mechanism::GenericSecretKeyGen, &template)?;
        session.close();
        Ok(())
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

        session
            .find_objects(&aes_template)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                error!(
                    "AES wrapping key '{}' not found — run digg-hsm-keytool create-wrapping-key",
                    String::from_utf8_lossy(&self.wrap_key_alias)
                );
                Error::Pkcs11(
                    cryptoki::error::RvError::KeyHandleInvalid,
                    cryptoki::context::Function::FindObjects,
                )
            })
    }

    /// Create a persistent AES-256 wrapping key in the HSM.
    /// Called only by `digg-hsm-keytool create-wrapping-key`, never by the service.
    pub fn create_aes_wrapping_key(&self, label: &str) -> Result<(), Error> {
        let aes_template = vec![
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::ValueLen(32.into()), // 256-bit
            Attribute::Encrypt(true),
            Attribute::Decrypt(true),
            Attribute::Wrap(true),
            Attribute::Unwrap(true),
            Attribute::Label(label.as_bytes().to_vec()),
        ];
        let session = self.pkcs11.open_rw_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;
        session.generate_key(&Mechanism::AesKeyGen, &aes_template)?;
        session.close();
        Ok(())
    }

    pub fn wrap_private_key(
        &self,
        session: &Session,
        ec_private_key: ObjectHandle,
    ) -> Result<WrappedPrivateKey, Error> {
        let mechanism = Mechanism::AesKeyWrapPad;
        let wrapping_key = self.aes_wrapping_key(session)?;

        let wrapped = session.wrap_key(&mechanism, wrapping_key, ec_private_key)?;
        Ok(WrappedPrivateKey::new(wrapped))
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
        debug!(
            "Successfully generated EC key pair with label: {} {:?}",
            ec_public_key, wrapped_private_key
        );

        Ok(HsmKey {
            wrapped_private_key,
            public_key_jwk,
            wrap_key_label: String::from_utf8_lossy(&self.wrap_key_alias).into_owned(),
            created_at: chrono::Utc::now(),
        })
    }

    fn derive_key(
        &self,
        root_key_label: &str,
        domain_separator: &str,
    ) -> Result<DerivedSecret, Error> {
        let session = self.pkcs11.open_ro_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;

        let root_key = session
            .find_objects(&[
                Attribute::Class(ObjectClass::SECRET_KEY),
                Attribute::Label(root_key_label.as_bytes().to_vec()),
            ])?
            .into_iter()
            .next()
            .ok_or(Error::Pkcs11(
                cryptoki::error::RvError::KeyHandleInvalid,
                cryptoki::context::Function::FindObjects,
            ))?;

        let hmac = session.sign(
            &Mechanism::Sha512Hmac,
            root_key,
            domain_separator.as_bytes(),
        )?;

        session.close();
        Ok(DerivedSecret::new(hmac))
    }

    fn sign(&self, key: &HsmKey, sign_payload: &[u8]) -> Result<Vec<u8>, Error> {
        let session = self.pkcs11.open_rw_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;
        let private_key =
            self.unwrap_private_key(&session, key.wrapped_private_key.as_bytes().to_vec())?;
        let signature = session.sign(&Mechanism::Ecdsa, private_key, sign_payload)?;
        session.close();
        Ok(signature)
    }
}

impl Drop for HsmWrapper {
    fn drop(&mut self) {
        debug!("HsmWrapper dropped. PKCS#11 context will finalize when all Arcs are dropped.");
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
