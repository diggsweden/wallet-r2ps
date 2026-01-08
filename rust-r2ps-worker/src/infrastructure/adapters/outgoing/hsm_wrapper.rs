use crate::application::hsm_spi_port::HsmSpiPort;
use crate::domain::Curve;
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
use elliptic_curve::pkcs8::EncodePublicKey;
use p256::ecdsa::VerifyingKey;
use sha2::Sha256;
use std::env;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

pub struct HsmWrapper {
    pkcs11: Arc<Pkcs11>,
    slot: Slot,
    so_pin: Option<AuthPin>,
    user_pin: Option<AuthPin>,
    wrap_key_alias: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct HsmKey {
    pub wrapped_private_key: Vec<u8>,
    pub public_key_pem: String,
    pub kid: String,
    pub curve_name: Curve,
    pub creation_time: chrono::DateTime<chrono::Utc>,
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

        let wrap_key_alias = config.wrap_key_alias.as_bytes().to_vec().into();

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
        match self.user_pin {
            Some(ref user_pin) => {
                session.init_pin(user_pin)?;
            }
            None => {}
        }
        Ok(())
    }

    pub fn ec_key_templates(&self, label: &str, curve: &Curve) -> (Vec<Attribute>, Vec<Attribute>) {
        let ec_params = match curve {
            &Curve::P256 => vec![0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07],
            &Curve::P384 => vec![0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x22],
            &Curve::P521 => vec![0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x23],
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
        let wrapping_key = self.aes_wrapping_key(&session)?;

        session.wrap_key(&mechanism, wrapping_key, ec_private_key)
    }

    pub fn unwrap_private_key(
        &self,
        session: &Session,
        wrapped_private_key: Vec<u8>,
    ) -> Result<ObjectHandle, Error> {
        let mechanism = Mechanism::AesKeyWrapPad;
        let wrap_key = self.aes_wrapping_key(&session)?;

        session.unwrap_key(
            &mechanism,
            wrap_key,
            &wrapped_private_key,
            &vec![
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

    pub fn get_ec_public_key(
        &self,
        session: &Session,
        public_key: ObjectHandle,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let attrs = session.get_attributes(public_key, &[AttributeType::EcPoint])?;

        for attr in attrs {
            if let Attribute::EcPoint(point) = attr {
                let octet_string = OctetStringRef::from_der(&point).map_err(|e| e.to_string())?;

                let verifying_key = VerifyingKey::from_sec1_bytes(octet_string.as_bytes())
                    .map_err(|e| e.to_string())?;
                let pem = verifying_key
                    .to_public_key_pem(Default::default())
                    .map_err(|e| e.to_string())?;

                return Ok(pem);
            }
        }

        Err("EC point not found".into())
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

        let public_key_pem = self.get_ec_public_key(&session, ec_public_key)?;

        session.close();
        println!(
            "Successfully generated EC key pair with label: {} {}",
            ec_public_key,
            wrapped_private_key.len()
        );

        Ok(HsmKey {
            wrapped_private_key,
            public_key_pem: public_key_pem.clone(),
            kid: Uuid::new_v4().to_string(),
            curve_name: Curve::P256,
            creation_time: chrono::Utc::now(),
        })
    }

    fn sign(&self, wrapped_key: &Vec<u8>, sign_payload: &Vec<u8>) -> Result<Vec<u8>, Error> {
        let session = self.pkcs11.open_rw_session(self.slot)?;
        session.login(UserType::User, self.user_pin.as_ref())?;
        let private_key = self.unwrap_private_key(&session, wrapped_key.clone())?;
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

impl HsmKey {
    pub fn new(wrapped_private_key: Vec<u8>, public_key_pem: String) -> Result<HsmKey, Error> {
        Ok(HsmKey {
            wrapped_private_key,
            public_key_pem,
            kid: uuid::Uuid::new_v4().to_string(),
            curve_name: Curve::P256,
            creation_time: Default::default(),
        })
    }
}

fn kid_from_pem(pem_bytes: &[u8]) -> String {
    // SHA-256 thumbprint of the DER-encoded public key
    let mut hasher = Sha256::new();
    hasher.update(pem_bytes); // or the actual DER bytes
    let hash = hasher.finalize();

    // Base64url encode (common for JWK thumbprints)
    URL_SAFE_NO_PAD.encode(&hash[..16]) // truncate or use full hash
}
