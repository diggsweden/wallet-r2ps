use josekit::jwe::ECDH_ES;
use josekit::jwe::JweHeader;
use p256::SecretKey;
use p256::pkcs8::{EncodePrivateKey, EncodePublicKey};
use rust_r2ps_worker::application::port::outgoing::jose_port::JosePort;
use rust_r2ps_worker::application::port::outgoing::jose_port::JweDecryptionKey;
use rust_r2ps_worker::domain::value_objects::TypedJwe;
use rust_r2ps_worker::domain::value_objects::r2ps::{InnerRequest, OperationId, OuterRequest};
use rust_r2ps_worker::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;

fn make_device_jwe(public_key_pem_str: &str, inner_request: &InnerRequest) -> String {
    let payload = serde_json::to_vec(inner_request).unwrap();
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");
    header.set_key_id("device");
    let encrypter = ECDH_ES.encrypter_from_pem(public_key_pem_str).unwrap();
    josekit::jwe::serialize_compact(&payload, &header, &encrypter).unwrap()
}

#[test]
fn test_decrypt_service_data_jwe_happy_path() -> Result<(), Box<dyn std::error::Error>> {
    // Generate an EC key pair (P-256) for the server
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let private_pem_str = secret_key
        .to_pkcs8_pem(Default::default())
        .map_err(|e| format!("Failed to generate PEM: {}", e))?;
    let public_pem_str = secret_key
        .public_key()
        .to_public_key_pem(Default::default())
        .map_err(|e| format!("Failed to generate public key PEM: {}", e))?;
    let private_pem = pem::parse(private_pem_str.as_bytes())?;
    let public_pem = pem::parse(public_pem_str.as_bytes())?;
    let jose = JoseAdapter::new(&public_pem, &private_pem).unwrap();

    // Create a valid InnerRequest instance
    let inner_request = InnerRequest {
        version: 1,
        request_type: OperationId::Info,
        request_counter: 0,
        data: Some("Hello, World! This is a secret message.".to_string()),
    };
    let jwe_compact = make_device_jwe(&public_pem_str, &inner_request);

    // Wrap the Base64 encoded payload in a ServiceRequest
    let service_request = OuterRequest {
        version: 1,
        session_id: None,
        context: "test-context".to_string(),
        inner_jwe: Some(TypedJwe::new(jwe_compact.clone())),
    };

    // Decrypt the serviceRequest with the private key
    let result_bytes = jose.jwe_decrypt(&jwe_compact, JweDecryptionKey::Device);
    assert!(
        result_bytes.is_ok(),
        "jwe_decrypt failed: {:?}",
        result_bytes.err()
    );

    let decoded: InnerRequest = serde_json::from_slice(&result_bytes.unwrap())?;
    assert_eq!(decoded.version, 1);
    assert_eq!(
        decoded.data.unwrap(),
        "Hello, World! This is a secret message."
    );

    // verify the JWE is also present in the outer request
    assert!(service_request.inner_jwe.is_some());

    Ok(())
}

#[test]
fn test_decrypt_service_data_jwe_rejects_invalid_formats() -> Result<(), Box<dyn std::error::Error>>
{
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let private_pem_str = secret_key
        .to_pkcs8_pem(Default::default())
        .map_err(|e| format!("Failed to generate PEM: {}", e))?;
    let public_pem_str = secret_key
        .public_key()
        .to_public_key_pem(Default::default())
        .map_err(|e| format!("Failed to generate public key PEM: {}", e))?;
    let private_pem = pem::parse(private_pem_str.as_bytes())?;
    let public_pem = pem::parse(public_pem_str.as_bytes())?;
    let jose = JoseAdapter::new(&public_pem, &private_pem).unwrap();

    let invalid_formats = vec![
        "only.four.parts.here",
        "this.is.way.too.many.parts",
        "missing_dots_in_this",
        "",
        "only.three.parts",
    ];

    for invalid_jwe in invalid_formats {
        let result = jose.jwe_decrypt(invalid_jwe, JweDecryptionKey::Device);
        assert!(
            result.is_err(),
            "Should reject invalid JWE: {:?}",
            invalid_jwe
        );
    }
    Ok(())
}

#[test]
fn test_decrypt_service_data_jwe_rejects_invalid_base64() -> Result<(), Box<dyn std::error::Error>>
{
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let private_pem_str = secret_key
        .to_pkcs8_pem(Default::default())
        .map_err(|e| format!("Failed to generate PEM: {}", e))?;
    let public_pem_str = secret_key
        .public_key()
        .to_public_key_pem(Default::default())
        .map_err(|e| format!("Failed to generate public key PEM: {}", e))?;
    let private_pem = pem::parse(private_pem_str.as_bytes())?;
    let public_pem = pem::parse(public_pem_str.as_bytes())?;
    let jose = JoseAdapter::new(&public_pem, &private_pem).unwrap();

    let result = jose.jwe_decrypt("not-base64!!", JweDecryptionKey::Device);
    assert!(result.is_err(), "jwe_decrypt should reject invalid base64");

    Ok(())
}
