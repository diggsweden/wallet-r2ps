use crate::domain::{
    DeviceHsmState, DeviceKeyEntry, EcPublicJwk, HsmKey, PasswordFile, PasswordFileEntry,
    ServiceRequestError, WrappedPrivateKey,
};
use rstest::rstest;

fn make_ec_jwk(kid: &str) -> EcPublicJwk {
    EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: "x-coord".to_string(),
        y: "y-coord".to_string(),
        kid: kid.to_string(),
    }
}

fn make_device_key(kid: &str) -> DeviceKeyEntry {
    DeviceKeyEntry {
        public_key: make_ec_jwk(kid),
        password_files: vec![],
        dev_authorization_code: None,
    }
}

fn make_device_key_with_auth_code(kid: &str, code: &str) -> DeviceKeyEntry {
    DeviceKeyEntry {
        public_key: make_ec_jwk(kid),
        password_files: vec![],
        dev_authorization_code: Some(code.to_string()),
    }
}

fn make_hsm_key(kid: &str) -> HsmKey {
    HsmKey {
        wrapped_private_key: WrappedPrivateKey::new(vec![0xDE, 0xAD]),
        public_key_jwk: make_ec_jwk(kid),
        wrap_key_label: "test-wrap-key".to_string(),
        created_at: chrono::Utc::now(),
    }
}

fn make_state() -> DeviceHsmState {
    DeviceHsmState {
        version: 1,
        device_keys: vec![],
        hsm_keys: vec![],
    }
}

fn make_password_file_entry() -> PasswordFileEntry {
    PasswordFileEntry {
        password_file: PasswordFile(vec![0x01, 0x02, 0x03]),
        opaque_domain_separator: "rk-202501_opaque-202501".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
    }
}

// === add_device_key ===

#[test]
fn add_device_key_accepts_valid_entry() {
    let mut state = make_state();
    let key = make_device_key("k1");
    let result = state.add_device_key(key);
    assert!(result.is_ok());
    assert_eq!(state.device_keys.len(), 1);
    assert!(state.find_device_key("k1").is_some());
}

#[rstest]
#[case("empty_kid", "", false, ServiceRequestError::InvalidPublicKey)]
#[case("duplicate_kid", "k1", true, ServiceRequestError::DuplicateKey)]
fn add_device_key_rejects_invalid(
    #[case] _name: &str,
    #[case] kid: &str,
    #[case] pre_insert: bool,
    #[case] expected_err: ServiceRequestError,
) {
    let mut state = make_state();
    if pre_insert {
        state.add_device_key(make_device_key(kid)).unwrap();
    }
    let result = state.add_device_key(make_device_key(kid));
    assert_eq!(result.unwrap_err(), expected_err);
}

#[test]
fn add_device_key_allows_different_kids() {
    let mut state = make_state();
    assert!(state.add_device_key(make_device_key("a")).is_ok());
    assert!(state.add_device_key(make_device_key("b")).is_ok());
    assert_eq!(state.device_keys.len(), 2);
}

// === remove_device_key ===

#[test]
fn remove_device_key_returns_entry() {
    let mut state = make_state();
    state.add_device_key(make_device_key("k1")).unwrap();
    let result = state.remove_device_key("k1");
    assert!(result.is_ok());
    let entry = result.unwrap();
    assert_eq!(entry.kid(), "k1");
    assert!(state.device_keys.is_empty());
}

#[test]
fn remove_device_key_unknown_kid() {
    let mut state = make_state();
    let result = state.remove_device_key("missing");
    assert!(matches!(result, Err(ServiceRequestError::UnknownClient)));
}

#[test]
fn remove_device_key_does_not_affect_others() {
    let mut state = make_state();
    state.add_device_key(make_device_key("a")).unwrap();
    state.add_device_key(make_device_key("b")).unwrap();
    state.add_device_key(make_device_key("c")).unwrap();
    state.remove_device_key("b").unwrap();
    assert_eq!(state.device_keys.len(), 2);
    assert_eq!(state.device_keys[0].kid(), "a");
    assert_eq!(state.device_keys[1].kid(), "c");
}

// === find_device_key / find_device_key_mut ===

#[test]
fn find_device_key_returns_some() {
    let mut state = make_state();
    state.add_device_key(make_device_key("k1")).unwrap();
    let found = state.find_device_key("k1");
    assert!(found.is_some());
    assert_eq!(found.unwrap().kid(), "k1");
}

#[test]
fn find_device_key_returns_none() {
    let state = make_state();
    assert!(state.find_device_key("missing").is_none());
}

#[test]
fn find_device_key_mut_returns_some() {
    let mut state = make_state();
    state.add_device_key(make_device_key("k1")).unwrap();
    {
        let entry = state.find_device_key_mut("k1").unwrap();
        entry.dev_authorization_code = Some("x".to_string());
    }
    let found = state.find_device_key("k1").unwrap();
    assert_eq!(found.dev_authorization_code.as_deref(), Some("x"));
}

#[test]
fn find_device_key_mut_returns_none() {
    let mut state = make_state();
    assert!(state.find_device_key_mut("missing").is_none());
}

// === add_hsm_key ===

#[test]
fn add_hsm_key_accepts_valid_key() {
    let mut state = make_state();
    let result = state.add_hsm_key(make_hsm_key("hk1"));
    assert!(result.is_ok());
    assert_eq!(state.hsm_keys.len(), 1);
}

#[rstest]
#[case("empty_kid", "", false, ServiceRequestError::InvalidPublicKey)]
#[case("duplicate_kid", "hk1", true, ServiceRequestError::DuplicateKey)]
fn add_hsm_key_rejects_invalid(
    #[case] _name: &str,
    #[case] kid: &str,
    #[case] pre_insert: bool,
    #[case] expected_err: ServiceRequestError,
) {
    let mut state = make_state();
    if pre_insert {
        state.add_hsm_key(make_hsm_key(kid)).unwrap();
    }
    let result = state.add_hsm_key(make_hsm_key(kid));
    assert_eq!(result.unwrap_err(), expected_err);
}

// === remove_hsm_key ===

#[test]
fn remove_hsm_key_returns_key() {
    let mut state = make_state();
    state.add_hsm_key(make_hsm_key("hk1")).unwrap();
    let result = state.remove_hsm_key("hk1");
    assert!(result.is_ok());
    let key = result.unwrap();
    assert_eq!(key.kid(), "hk1");
    assert!(state.hsm_keys.is_empty());
}

#[test]
fn remove_hsm_key_not_found() {
    let mut state = make_state();
    let result = state.remove_hsm_key("missing");
    assert!(matches!(result, Err(ServiceRequestError::HsmKeyNotFound)));
}

// === find_hsm_key ===

#[test]
fn find_hsm_key_returns_some() {
    let mut state = make_state();
    state.add_hsm_key(make_hsm_key("hk1")).unwrap();
    assert!(state.find_hsm_key("hk1").is_some());
}

#[test]
fn find_hsm_key_returns_none() {
    let state = make_state();
    assert!(state.find_hsm_key("missing").is_none());
}

// === set_password_file ===

/// `entry_auth_code`: auth code pre-set on the device key entry (None = no code)
/// `provided_code`: code passed to `set_password_file`
/// `try_reuse`: if true, consume the code first then retry, expecting the error on the second call
#[rstest]
#[case(
    "unknown_kid",
    false,
    None,
    None,
    false,
    Err(ServiceRequestError::UnknownClient)
)]
#[case("no_auth_code",      true,  None,          None,            false, Ok(()))]
#[case("valid_auth_code",   true,  Some("secret"),Some("secret"),  false, Ok(()))]
#[case(
    "wrong_auth_code",
    true,
    Some("secret"),
    Some("wrong"),
    false,
    Err(ServiceRequestError::InvalidAuthorizationCode)
)]
#[case(
    "reuse_consumed",
    true,
    Some("secret"),
    Some("secret"),
    true,
    Err(ServiceRequestError::InvalidAuthorizationCode)
)]
#[case("no_param_skips",    true,  Some("secret"),None,            false, Ok(()))]
fn set_password_file_cases(
    #[case] _name: &str,
    #[case] add_key: bool,
    #[case] entry_auth_code: Option<&str>,
    #[case] provided_code: Option<&str>,
    #[case] try_reuse: bool,
    #[case] expected: Result<(), ServiceRequestError>,
) {
    let mut state = make_state();
    if add_key {
        let key = match entry_auth_code {
            Some(code) => make_device_key_with_auth_code("k1", code),
            None => make_device_key("k1"),
        };
        state.add_device_key(key).unwrap();
    }
    let kid = if add_key { "k1" } else { "missing" };
    if try_reuse {
        // consume the code first, then the second call should fail
        state
            .set_password_file(kid, make_password_file_entry(), provided_code)
            .unwrap();
    }
    let result = state.set_password_file(kid, make_password_file_entry(), provided_code);
    assert_eq!(result, expected);
}

#[test]
fn set_password_file_no_auth_code_replaces() {
    let mut state = make_state();
    state.add_device_key(make_device_key("k1")).unwrap();
    state
        .set_password_file("k1", make_password_file_entry(), None)
        .unwrap();
    assert_eq!(state.find_device_key("k1").unwrap().password_files.len(), 1);
    state
        .set_password_file("k1", make_password_file_entry(), None)
        .unwrap();
    assert_eq!(state.find_device_key("k1").unwrap().password_files.len(), 1);
}

#[test]
fn set_password_file_with_valid_auth_code_clears_code() {
    let mut state = make_state();
    state
        .add_device_key(make_device_key_with_auth_code("k1", "secret"))
        .unwrap();
    state
        .set_password_file("k1", make_password_file_entry(), Some("secret"))
        .unwrap();
    let entry = state.find_device_key("k1").unwrap();
    assert!(entry.dev_authorization_code.is_none());
    assert_eq!(entry.password_files.len(), 1);
}

#[test]
fn set_password_file_no_code_param_preserves_code() {
    let mut state = make_state();
    state
        .add_device_key(make_device_key_with_auth_code("k1", "secret"))
        .unwrap();
    state
        .set_password_file("k1", make_password_file_entry(), None)
        .unwrap();
    let entry = state.find_device_key("k1").unwrap();
    assert_eq!(entry.dev_authorization_code.as_deref(), Some("secret"));
}

// === get_password_file ===

#[test]
fn get_password_file_kid_absent() {
    let state = make_state();
    assert!(state.get_password_file("missing").is_none());
}

#[test]
fn get_password_file_empty_password_files() {
    let mut state = make_state();
    state.add_device_key(make_device_key("k1")).unwrap();
    assert!(state.get_password_file("k1").is_none());
}

#[test]
fn get_password_file_returns_last() {
    let mut state = make_state();
    state.add_device_key(make_device_key("k1")).unwrap();
    let first_entry = PasswordFileEntry {
        password_file: PasswordFile(vec![0xAA, 0xBB]),
        opaque_domain_separator: "rk-202501_opaque-202501".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let second_entry = PasswordFileEntry {
        password_file: PasswordFile(vec![0xCC, 0xDD]),
        opaque_domain_separator: "rk-202501_opaque-202502".to_string(),
        created_at: "2026-01-02T00:00:00Z".to_string(),
    };
    {
        let entry = state.find_device_key_mut("k1").unwrap();
        entry.password_files.push(first_entry);
        entry.password_files.push(second_entry);
    }
    let pf = state.get_password_file("k1").unwrap();
    assert_eq!(pf.as_bytes(), &[0xCC, 0xDD]);
}

// === Serialization ===

#[test]
fn serialize_roundtrip() {
    let mut state = make_state();
    state.add_device_key(make_device_key("dk1")).unwrap();
    {
        let entry = state.find_device_key_mut("dk1").unwrap();
        entry.password_files.push(make_password_file_entry());
    }
    state.add_hsm_key(make_hsm_key("hk1")).unwrap();

    let bytes = state.serialize();
    assert!(bytes.is_ok());
    let deserialized = serde_json::from_slice::<DeviceHsmState>(&bytes.unwrap());
    assert!(deserialized.is_ok());
    let restored = deserialized.unwrap();
    assert_eq!(restored.version, 1);
    assert_eq!(restored.device_keys.len(), 1);
    assert_eq!(restored.hsm_keys.len(), 1);
    assert_eq!(restored.device_keys[0].kid(), "dk1");
    assert_eq!(restored.hsm_keys[0].kid(), "hk1");
}

#[test]
fn serialize_empty_state() {
    let state = make_state();
    let bytes = state.serialize();
    assert!(bytes.is_ok());
    let deserialized = serde_json::from_slice::<DeviceHsmState>(&bytes.unwrap());
    assert!(deserialized.is_ok());
    let restored = deserialized.unwrap();
    assert!(restored.device_keys.is_empty());
    assert!(restored.hsm_keys.is_empty());
}
