//! Test-only helpers: build a real 2-cert DER chain (leaf→root) bound to a P-256 key.

use p256::pkcs8::EncodePrivateKey;
use p256::{PublicKey, SecretKey};
use rand_core::OsRng;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
    PKCS_ECDSA_P256_SHA256,
};
use time::{Duration, OffsetDateTime};

pub struct TestChain {
    /// Concatenated DER: leaf ++ ca (leaf→root).
    pub der: Vec<u8>,
    pub secret: SecretKey,
    pub public: PublicKey,
}

fn dn(cn: &str) -> DistinguishedName {
    let mut d = DistinguishedName::new();
    d.push(DnType::CommonName, cn);
    d
}

/// Build a self-signed CA and a leaf (holding `secret`) signed by it.
pub fn test_chain() -> TestChain {
    let now = OffsetDateTime::now_utc();

    let ca_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.distinguished_name = dn("RKP Mini Test CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.not_before = now - Duration::days(1);
    ca_params.not_after = now + Duration::days(3650);
    let ca = ca_params.self_signed(&ca_key).unwrap();

    let secret = SecretKey::random(&mut OsRng);
    let public = secret.public_key();
    let pkcs8 = secret.to_pkcs8_der().unwrap();
    let leaf_key = KeyPair::try_from(pkcs8.as_bytes()).unwrap();

    let mut leaf_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    leaf_params.distinguished_name = dn("RKP Mini Test Leaf");
    leaf_params.not_before = now - Duration::days(1);
    leaf_params.not_after = now + Duration::days(30);
    let leaf = leaf_params.signed_by(&leaf_key, &ca, &ca_key).unwrap();

    let mut der = leaf.der().as_ref().to_vec();
    der.extend_from_slice(ca.der().as_ref());

    TestChain {
        der,
        secret,
        public,
    }
}
