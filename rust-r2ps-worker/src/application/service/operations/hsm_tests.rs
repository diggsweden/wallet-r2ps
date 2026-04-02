#[cfg(test)]
mod tests {
    use crate::application::hsm_spi_port::MockHsmSpiPort;
    use crate::application::service::operations::hsm::{
        HsmDeleteKeyOperation, HsmGenerateKeyOperation, HsmListKeysOperation, HsmSignOperation,
        MessageVector,
    };
    use crate::application::service::operations::{OperationContext, ServiceOperation};
    use crate::domain::{
        CreateKeyServiceData, Curve, DeleteKeyServiceData, DeviceHsmState, DeviceKeyEntry,
        EcPublicJwk, HsmKey, InnerRequest, ListKeysResponse, OuterRequest, ServiceRequestError,
        SignRequest, SignatureResponse, WrappedPrivateKey,
    };
    use std::sync::Arc;

    // -----------------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------------

    fn create_mock_context(state: DeviceHsmState, inner_request: InnerRequest) -> OperationContext {
        OperationContext {
            request_id: "test-request".to_string(),
            state,
            outer_request: OuterRequest {
                version: 1,
                session_id: None,
                context: "hsm".to_string(),
                inner_jwe: None, // Simplified for testing HSM ops
                server_kid: None,
            },
            inner_request,
            session_id: None,
            device_kid: "device-key-1".to_string(),
            session_state: None,
        }
    }

    fn make_hsm_key(kid: &str) -> HsmKey {
        HsmKey {
            wrapped_private_key: WrappedPrivateKey::new(vec![1, 2, 3]),
            public_key_jwk: EcPublicJwk {
                kty: "EC".to_string(),
                crv: "P-256".to_string(),
                x: "x".to_string(),
                y: "y".to_string(),
                kid: kid.to_string(),
            },
            wrap_key_label: String::new(),
            created_at: chrono::Utc::now(),
        }
    }

    fn initial_state() -> DeviceHsmState {
        DeviceHsmState {
            version: 1,
            device_keys: vec![DeviceKeyEntry {
                public_key: EcPublicJwk {
                    kty: "EC".to_string(),
                    crv: "P-256".to_string(),
                    x: "x".to_string(),
                    y: "y".to_string(),
                    kid: "device-key-1".to_string(),
                },
                password_files: vec![],
                dev_authorization_code: None,
            }],
            hsm_keys: vec![],
        }
    }

    // -----------------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------------

    #[test]
    fn test_hsm_generate_key_operation_state_mutation() {
        let mut mock_hsm = MockHsmSpiPort::new();
        mock_hsm.expect_generate_key().once().returning(|_, _| {
            Ok(HsmKey {
                wrapped_private_key: WrappedPrivateKey::new(vec![1, 2, 3]),
                public_key_jwk: EcPublicJwk {
                    kty: "EC".to_string(),
                    crv: "P-256".to_string(),
                    x: "x".to_string(),
                    y: "y".to_string(),
                    kid: "new-hsm-key-id".to_string(),
                },
                wrap_key_label: String::new(),
                created_at: chrono::Utc::now(),
            })
        });
        let op = HsmGenerateKeyOperation::new(Arc::new(mock_hsm));

        let payload = CreateKeyServiceData { curve: Curve::P256 };
        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmGenerateKey,
            request_counter: 0,
            data: Some(serde_json::to_string(&payload).unwrap()),
        };

        let context = create_mock_context(initial_state(), inner_request);

        let result = op.execute(context).unwrap();

        // Verify state mutation
        let new_state = result.state.expect("Should return updated state");
        assert_eq!(new_state.hsm_keys.len(), 1);
        assert_eq!(new_state.hsm_keys[0].public_key_jwk.kid, "new-hsm-key-id");

        // Verify response data contains the new key ID
        let json = String::from_utf8(result.data.serialize().unwrap()).unwrap();
        assert!(json.contains("new-hsm-key-id"));
    }

    #[test]
    fn test_hsm_sign_operation_success() {
        let mut mock_hsm = MockHsmSpiPort::new();
        mock_hsm
            .expect_sign()
            .once()
            // Return a valid dummy 64-byte signature (R | S) for P-256
            // R and S must be non-zero and within (1, n-1)
            .returning(|_, _| Ok(vec![1u8; 64]));
        let op = HsmSignOperation::new(Arc::new(mock_hsm));

        // Set up state with an existing HSM key
        let mut state = initial_state();
        state.hsm_keys.push(make_hsm_key("hsm-key-1"));

        let payload = SignRequest {
            hsm_kid: "hsm-key-1".to_string(),
            message: MessageVector::new(vec![1, 2, 3, 4]),
        };
        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmSign,
            request_counter: 0,
            data: Some(serde_json::to_string(&payload).unwrap()),
        };

        let context = create_mock_context(state, inner_request);

        let result = op.execute(context).unwrap();

        // Verify no state mutation (Sign doesn't change state)
        assert!(result.state.is_none());

        // Verify response contains a valid DER-encoded P-256 signature
        let serialized = result.data.serialize().unwrap();
        let response: SignatureResponse = serde_json::from_slice(&serialized).unwrap();
        p256::ecdsa::Signature::from_der(&response.signature)
            .expect("response should contain a valid DER-encoded P-256 signature");
    }

    #[test]
    fn test_hsm_sign_operation_unknown_key_fails() {
        let op = HsmSignOperation::new(Arc::new(MockHsmSpiPort::new()));

        let payload = SignRequest {
            hsm_kid: "non-existent-key".to_string(),
            message: MessageVector::new(vec![1, 2, 3, 4]),
        };
        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmSign,
            request_counter: 0,
            data: Some(serde_json::to_string(&payload).unwrap()),
        };

        let context = create_mock_context(initial_state(), inner_request);

        let result = op.execute(context);

        assert!(matches!(result, Err(ServiceRequestError::UnknownKey)));
    }

    #[test]
    fn hsm_sign_returns_unknown_when_hsm_returns_non_p256_signature_bytes() {
        let mut mock_hsm = MockHsmSpiPort::new();
        mock_hsm
            .expect_sign()
            .once()
            // Return wrong-length bytes — p256::ecdsa::Signature::from_slice requires exactly 64
            .returning(|_, _| Ok(vec![0xAB; 10]));
        let op = HsmSignOperation::new(Arc::new(mock_hsm));

        let mut state = initial_state();
        state.hsm_keys.push(make_hsm_key("hsm-key-1"));

        let payload = SignRequest {
            hsm_kid: "hsm-key-1".to_string(),
            message: MessageVector::new(vec![1, 2, 3, 4]),
        };
        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmSign,
            request_counter: 0,
            data: Some(serde_json::to_string(&payload).unwrap()),
        };

        let context = create_mock_context(state, inner_request);

        let result = op.execute(context);

        assert!(matches!(result, Err(ServiceRequestError::Unknown)));
    }

    #[test]
    fn test_hsm_delete_key_operation_state_mutation() {
        let op = HsmDeleteKeyOperation;

        let mut state = initial_state();
        state.hsm_keys.push(make_hsm_key("hsm-key-1"));

        let payload = DeleteKeyServiceData {
            hsm_kid: "hsm-key-1".to_string(),
        };
        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmDeleteKey,
            request_counter: 0,
            data: Some(serde_json::to_string(&payload).unwrap()),
        };

        let context = create_mock_context(state, inner_request);

        let result = op.execute(context).unwrap();

        // Verify key was removed from state
        let new_state = result.state.expect("Should return updated state");
        assert!(new_state.hsm_keys.is_empty());

        // Verify response echoes the deleted key ID
        let json = String::from_utf8(result.data.serialize().unwrap()).unwrap();
        assert!(json.contains("hsm-key-1"));
    }

    #[test]
    fn test_hsm_delete_key_operation_unknown_key_fails() {
        let op = HsmDeleteKeyOperation;

        let payload = DeleteKeyServiceData {
            hsm_kid: "non-existent-key".to_string(),
        };
        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmDeleteKey,
            request_counter: 0,
            data: Some(serde_json::to_string(&payload).unwrap()),
        };

        let context = create_mock_context(initial_state(), inner_request);

        let result = op.execute(context);

        assert!(matches!(result, Err(ServiceRequestError::HsmKeyNotFound)));
    }

    #[test]
    fn test_hsm_list_keys_operation_returns_all_keys() {
        let op = HsmListKeysOperation;

        let mut state = initial_state();
        state.hsm_keys.push(make_hsm_key("hsm-key-1"));
        state.hsm_keys.push(make_hsm_key("hsm-key-2"));

        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmListKeys,
            request_counter: 0,
            data: None,
        };

        let context = create_mock_context(state, inner_request);

        let result = op.execute(context).unwrap();

        // Verify no state mutation (list is read-only)
        assert!(result.state.is_none());

        // Verify response contains both keys
        let serialized = result.data.serialize().unwrap();
        let response: ListKeysResponse = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(response.key_info.len(), 2);
        let kids: Vec<&str> = response
            .key_info
            .iter()
            .map(|k| k.public_key.kid.as_str())
            .collect();
        assert!(kids.contains(&"hsm-key-1"));
        assert!(kids.contains(&"hsm-key-2"));
    }

    #[test]
    fn test_hsm_list_keys_operation_empty_state() {
        let op = HsmListKeysOperation;

        let inner_request = InnerRequest {
            version: 1,
            request_type: crate::domain::OperationId::HsmListKeys,
            request_counter: 0,
            data: None,
        };

        let context = create_mock_context(initial_state(), inner_request);

        let result = op.execute(context).unwrap();

        let serialized = result.data.serialize().unwrap();
        let response: ListKeysResponse = serde_json::from_slice(&serialized).unwrap();
        assert!(response.key_info.is_empty());
    }
}
