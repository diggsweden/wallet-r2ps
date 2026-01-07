use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use josekit::jwe::ECDH_ES;
use josekit::jwe::JweHeader;
use p256::SecretKey;
use p256::pkcs8::{EncodePrivateKey, EncodePublicKey};
use rust_r2ps_worker::application::service::r2ps_service::decrypt_service_data_jwe;
use rust_r2ps_worker::domain::ServiceRequestError;
use rust_r2ps_worker::domain::value_objects::r2ps::{ServiceRequest, ServiceTypeId};

#[test]
fn test_decrypt_service_data_jwe_happy_path() -> Result<(), Box<dyn std::error::Error>> {
    // Generate an EC key pair (P-256) for the server
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let private_key_pem_string = secret_key
        .to_pkcs8_pem(Default::default())
        .map_err(|e| format!("Failed to generate PEM: {}", e))?;
    let server_private_key = pem::parse(private_key_pem_string.as_bytes())?;

    let public_key_pem_string = secret_key
        .public_key()
        .to_public_key_pem(Default::default())
        .map_err(|e| format!("Failed to generate public key PEM: {}", e))?;

    // Encrypt a payload with the public key
    let payload = b"Hello, World! This is a secret message.";

    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");

    let encrypter = ECDH_ES.encrypter_from_pem(&public_key_pem_string)?;
    let jwe_compact = josekit::jwe::serialize_compact(payload, &header, &encrypter)?;

    // Wrap the Base64 encoded payload in a ServiceRequest
    let service_request = ServiceRequest {
        client_id: "test-client".to_string(),
        kid: "test-kid".to_string(),
        context: "test-context".to_string(),
        service_type: ServiceTypeId::Authenticate,
        pake_session_id: None,
        version: None,
        nonce: None,
        iat: None,
        enc: Some("ECDH-ES".to_string()),
        service_data: Some(BASE64_STANDARD.encode(jwe_compact)),
    };

    // Decrypt the serviceRequest with the private key
    let result_new = decrypt_service_data_jwe(&service_request, &server_private_key);
    assert!(
        result_new.is_ok(),
        "decrypt_service_data_jwe failed: {:?}",
        result_new.err()
    );
    assert_eq!(result_new.unwrap(), payload);

    Ok(())
}

#[test]
fn test_decrypt_service_data_jwe_rejects_invalid_formats() -> Result<(), Box<dyn std::error::Error>>
{
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let private_key_pem_string = secret_key
        .to_pkcs8_pem(Default::default())
        .map_err(|e| format!("Failed to generate PEM: {}", e))?;
    let server_private_key = pem::parse(private_key_pem_string.as_bytes())?;

    let invalid_formats = vec![
        "only.four.parts.here",
        "this.is.way.too.many.parts",
        "missing_dots_in_this",
        "",
        "only.three.parts",
    ];

    for invalid_jwe in invalid_formats {
        let service_request = ServiceRequest {
            client_id: "test-client".to_string(),
            kid: "test-kid".to_string(),
            context: "test-context".to_string(),
            service_type: ServiceTypeId::Authenticate,
            pake_session_id: None,
            version: None,
            nonce: None,
            iat: None,
            enc: Some("ECDH-ES".to_string()),
            service_data: Some(BASE64_STANDARD.encode(invalid_jwe)),
        };

        let result = decrypt_service_data_jwe(&service_request, &server_private_key);

        assert!(matches!(result, Err(ServiceRequestError::JweError)));
    }
    Ok(())
}

#[test]
fn test_decrypt_service_data_jwe_rejects_invalid_base64() -> Result<(), Box<dyn std::error::Error>>
{
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let private_key_pem_string = secret_key
        .to_pkcs8_pem(Default::default())
        .map_err(|e| format!("Failed to generate PEM: {}", e))?;
    let server_private_key = pem::parse(private_key_pem_string.as_bytes())?;

    let service_request = ServiceRequest {
        client_id: "test-client".to_string(),
        kid: "test-kid".to_string(),
        context: "test-context".to_string(),
        service_type: ServiceTypeId::Authenticate,
        pake_session_id: None,
        version: None,
        nonce: None,
        iat: None,
        enc: Some("ECDH-ES".to_string()),
        service_data: Some(BASE64_STANDARD.encode("not-base64!!".to_string())),
    };

    let result = decrypt_service_data_jwe(&service_request, &server_private_key);
    assert!(
        result.is_err(),
        "decrypt_service_data_jwe should reject invalid base64: {:?}",
        result
    );

    Ok(())
}
