#[cfg(test)]
mod tests {
    use crate::application::OuterError;
    use crate::application::port::outgoing::jose_port::{JoseError, MockJosePort};
    use crate::application::port::outgoing::session_state_spi_port::SessionKey;
    use crate::domain::{InnerRequest, OperationId, OuterRequest, SessionId, TypedJwe};
    use rstest::rstest;

    fn create_inner_request_json(operation: OperationId) -> Vec<u8> {
        let inner_request = InnerRequest {
            version: 1,
            request_type: operation,
            request_counter: 42,
            data: Some("test_data".to_string()),
        };
        serde_json::to_vec(&inner_request).unwrap()
    }

    fn create_outer_request_with_jwe(jwe: Option<String>) -> OuterRequest {
        OuterRequest {
            version: 1,
            session_id: Some(SessionId::new()),
            context: "hsm".to_string(),
            server_kid: None,
            inner_jwe: jwe.map(TypedJwe::new),
        }
    }

    #[test]
    fn test_decrypt_inner_missing_jwe() {
        let outer_request = create_outer_request_with_jwe(None);
        let mock_jose = MockJosePort::new();
        let session_key = SessionKey::new(vec![0u8; 32]);

        let result = outer_request.decrypt_inner(&mock_jose, Some(&session_key));

        assert!(matches!(result, Err(OuterError::InnerJweMissing)));
    }

    #[test]
    fn test_decrypt_inner_invalid_jwe_header() {
        let outer_request = create_outer_request_with_jwe(Some("malformed.jwe".to_string()));
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_peek_kid()
            .returning(|_| Err(JoseError::InvalidKey));
        let session_key = SessionKey::new(vec![0u8; 32]);

        let result = outer_request.decrypt_inner(&mock_jose, Some(&session_key));

        assert!(matches!(result, Err(OuterError::InnerJweHeaderInvalid)));
    }

    #[test]
    fn test_decrypt_inner_session_key_missing() {
        let outer_request = create_outer_request_with_jwe(Some("valid.jwe".to_string()));
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_peek_kid()
            .returning(|_| Ok(Some("session".to_string())));

        let result = outer_request.decrypt_inner(&mock_jose, None);

        assert!(matches!(result, Err(OuterError::SessionKeyMissing)));
    }

    #[rstest]
    #[case(Some("unknown".to_string()))]
    #[case(None)]
    fn test_decrypt_inner_unknown_encryption_option(#[case] kid: Option<String>) {
        let outer_request = create_outer_request_with_jwe(Some("valid.jwe".to_string()));
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_peek_kid()
            .returning(move |_| Ok(kid.clone()));
        let session_key = SessionKey::new(vec![0u8; 32]);

        let result = outer_request.decrypt_inner(&mock_jose, Some(&session_key));

        assert!(matches!(result, Err(OuterError::UnknownEncryptionOption)));
    }

    #[test]
    fn test_decrypt_inner_decryption_failed() {
        let outer_request = create_outer_request_with_jwe(Some("valid.jwe".to_string()));
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_peek_kid()
            .returning(|_| Ok(Some("session".to_string())));
        mock_jose
            .expect_jwe_decrypt()
            .returning(|_, _| Err(JoseError::DecryptError));
        let session_key = SessionKey::new(vec![0u8; 32]);

        let result = outer_request.decrypt_inner(&mock_jose, Some(&session_key));

        assert!(matches!(result, Err(OuterError::InnerJweDecryptFailed)));
    }

    #[test]
    fn test_decrypt_inner_invalid_json_after_decryption() {
        let outer_request = create_outer_request_with_jwe(Some("valid.jwe".to_string()));
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_peek_kid()
            .returning(|_| Ok(Some("session".to_string())));
        mock_jose
            .expect_jwe_decrypt()
            .returning(|_, _| Ok(b"not valid json".to_vec()));
        let session_key = SessionKey::new(vec![0u8; 32]);

        let result = outer_request.decrypt_inner(&mock_jose, Some(&session_key));

        assert!(matches!(result, Err(OuterError::InnerJweDecryptFailed)));
    }

    #[rstest]
    #[case(OperationId::AuthenticateStart, "session", true)]
    #[case(OperationId::RegisterStart, "session", true)]
    #[case(OperationId::HsmSign, "device", false)]
    #[case(OperationId::HsmListKeys, "device", false)]
    fn test_decrypt_inner_encryption_option_mismatch(
        #[case] operation: OperationId,
        #[case] kid: &str,
        #[case] needs_session_key: bool,
    ) {
        let inner_json = create_inner_request_json(operation);
        let kid = kid.to_string();

        let outer_request = create_outer_request_with_jwe(Some("valid.jwe".to_string()));
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_peek_kid()
            .returning(move |_| Ok(Some(kid.clone())));
        mock_jose
            .expect_jwe_decrypt()
            .returning(move |_, _| Ok(inner_json.clone()));

        let session_key = SessionKey::new(vec![0u8; 32]);
        let session_key_ref = if needs_session_key {
            Some(&session_key)
        } else {
            None
        };

        let result = outer_request.decrypt_inner(&mock_jose, session_key_ref);

        assert!(matches!(result, Err(OuterError::InnerJweDecryptFailed)));
    }

    #[rstest]
    #[case(OperationId::HsmListKeys, "session", true)]
    #[case(OperationId::HsmSign, "session", true)]
    #[case(OperationId::RegisterStart, "device", false)]
    #[case(OperationId::AuthenticateStart, "device", false)]
    fn test_decrypt_inner_success(
        #[case] operation: OperationId,
        #[case] kid: &str,
        #[case] needs_session_key: bool,
    ) {
        let inner_json = create_inner_request_json(operation);
        let kid = kid.to_string();

        let outer_request = create_outer_request_with_jwe(Some("valid.jwe".to_string()));
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_peek_kid()
            .returning(move |_| Ok(Some(kid.clone())));
        mock_jose
            .expect_jwe_decrypt()
            .returning(move |_, _| Ok(inner_json.clone()));

        let session_key = SessionKey::new(vec![0u8; 32]);
        let session_key_ref = if needs_session_key {
            Some(&session_key)
        } else {
            None
        };

        let result = outer_request.decrypt_inner(&mock_jose, session_key_ref);

        assert!(result.is_ok());
        let inner_request = result.unwrap();
        assert_eq!(inner_request.request_type, operation);
        assert_eq!(inner_request.version, 1);
        assert_eq!(inner_request.request_counter, 42);
    }
}
