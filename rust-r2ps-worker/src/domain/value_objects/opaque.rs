use opaque_ke::CipherSuite;
use opaque_ke::ksf::Identity;

#[derive(Clone, Copy)]
pub struct DefaultCipherSuite;

impl CipherSuite for DefaultCipherSuite {
    type OprfCs = p256::NistP256;
    type KeyExchange = opaque_ke::TripleDh<p256::NistP256, sha2::Sha256>;
    type Ksf = Identity;
}
