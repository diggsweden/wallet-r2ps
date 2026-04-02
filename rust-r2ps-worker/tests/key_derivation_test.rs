use cryptoki::error::Error as CryptokiError;
use rust_r2ps_worker::application::port::outgoing::hsm_spi_port::{DerivedSecret, HsmSpiPort};
use rust_r2ps_worker::domain::{Curve, HsmKey};
use rust_r2ps_worker::infrastructure::config::{jose_utils, key_derivation};
use sha2::{Digest, Sha512};

/// Mock HSM that computes SHA-512(root_key || domain_separator) as a stand-in for HMAC-SHA512.
struct MockHsmPort {
    root_key: [u8; 64],
}

impl HsmSpiPort for MockHsmPort {
    fn generate_key(
        &self,
        _label: &str,
        _curve: &Curve,
    ) -> Result<HsmKey, Box<dyn std::error::Error>> {
        unimplemented!()
    }

    fn sign(&self, _key: &HsmKey, _payload: &[u8]) -> Result<Vec<u8>, CryptokiError> {
        unimplemented!()
    }

    fn derive_key(
        &self,
        _root_key_label: &str,
        domain_separator: &str,
    ) -> Result<DerivedSecret, CryptokiError> {
        let output = Sha512::new()
            .chain_update(self.root_key)
            .chain_update(domain_separator.as_bytes())
            .finalize();
        Ok(DerivedSecret::new(output.to_vec()))
    }
}

fn full_derive(hsm: &dyn HsmSpiPort, root_label: &str, domain_sep: &str) -> p256::SecretKey {
    let prf_output = hsm
        .derive_key(root_label, domain_sep)
        .expect("derive_key failed");
    key_derivation::derive_scalar(prf_output.as_ref(), domain_sep).expect("derive_scalar failed")
}

#[test]
fn derivation_is_deterministic() {
    let hsm = MockHsmPort {
        root_key: [0x01u8; 64],
    };
    let k1 = full_derive(&hsm, "rk-test", "jws-v1");
    let k2 = full_derive(&hsm, "rk-test", "jws-v1");
    assert_eq!(k1.to_bytes(), k2.to_bytes());
}

#[test]
fn different_domain_seps_produce_different_keys() {
    let hsm = MockHsmPort {
        root_key: [0x01u8; 64],
    };
    let jws = full_derive(&hsm, "rk-test", "jws-v1");
    let opaque = full_derive(&hsm, "rk-test", "opaque-v1");
    assert_ne!(jws.to_bytes(), opaque.to_bytes());
}

#[test]
fn different_root_keys_produce_different_keys() {
    let hsm1 = MockHsmPort {
        root_key: [0x01u8; 64],
    };
    let hsm2 = MockHsmPort {
        root_key: [0x02u8; 64],
    };
    let k1 = full_derive(&hsm1, "rk-test", "jws-v1");
    let k2 = full_derive(&hsm2, "rk-test", "jws-v1");
    assert_ne!(k1.to_bytes(), k2.to_bytes());
}

#[test]
fn derived_key_is_valid_p256() {
    let hsm = MockHsmPort {
        root_key: [0xABu8; 64],
    };
    let key = full_derive(&hsm, "rk-test", "jws-v1");
    // Valid if public key computation and KID derivation succeed without panic
    let kid = jose_utils::ec_kid_from_secret(&key);
    assert!(!kid.is_empty());
}

#[test]
fn derive_scalar_domain_sep_changes_output() {
    let ikm = [0x42u8; 64];
    let k1 = key_derivation::derive_scalar(&ikm, "domain-a").unwrap();
    let k2 = key_derivation::derive_scalar(&ikm, "domain-b").unwrap();
    assert_ne!(k1.to_bytes(), k2.to_bytes());
}

#[test]
fn derive_scalar_ikm_changes_output() {
    let k1 = key_derivation::derive_scalar(&[0x01u8; 64], "jws-v1").unwrap();
    let k2 = key_derivation::derive_scalar(&[0x02u8; 64], "jws-v1").unwrap();
    assert_ne!(k1.to_bytes(), k2.to_bytes());
}

/// Test vectors: PRF = SHA-512(root_key || domain_sep), then derive_scalar → ec_kid_from_secret.
/// Each row verifies that changing the root key OR the domain separator changes the KID.
#[rstest::rstest]
#[case([0x01u8; 64], "jws-v1",    "REj5JviBwwtOUISuiorN_6by1Gm2d6gL2WTdhVYBY0c")]
#[case([0x01u8; 64], "opaque-v1", "5tfhYykGOczLhrrCNEneAFkEDBGuUMKeGyzGviFSJTQ")]
#[case([0x02u8; 64], "jws-v1",    "R9T3jRNECC5z2gJJE6XJOEYE-vmdjc9srALyE5FTIvw")]
#[case([0x02u8; 64], "opaque-v1", "147D-pVFNbCYtgcaUpwP8HHQQU_pczyisBHlu49XHzA")]
fn known_test_vector_kid(
    #[case] root_key: [u8; 64],
    #[case] domain_sep: &str,
    #[case] expected_kid: &str,
) {
    let hsm = MockHsmPort { root_key };
    let key = full_derive(&hsm, "rk-test", domain_sep);
    let kid = jose_utils::ec_kid_from_secret(&key);
    assert_eq!(kid, expected_kid);
}
