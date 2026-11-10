//! Test-only fixtures: real P-256 chains and keybox XML in the RKP Mini layout.

use base64::Engine;
use der::{asn1::ObjectIdentifier, Encode};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::EncodePrivateKey;
use p256::SecretKey;
use rand_core::OsRng;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
    PKCS_ECDSA_P256_SHA256,
};
use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;

/// secp256r1 (`1.2.840.10045.3.1.7`): the named curve OhMyKeymint's
/// `import_sec1_private_key` requires in the SEC1 `parameters` field.
const SECP256R1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");

pub struct TestKeybox {
    pub xml: String,
    pub key_pem: String,
    /// Leaf→root PEM blocks.
    pub chain_pems: Vec<String>,
    /// Concatenated DER, leaf→root.
    pub chain_der: Vec<u8>,
    pub device_id: String,
}

fn dn(cn: &str) -> DistinguishedName {
    let mut d = DistinguishedName::new();
    d.push(DnType::CommonName, cn);
    d
}

/// SEC1 `ECPrivateKey` DER with the secp256r1 named curve in `parameters`.
///
/// Mirrors what `rkp-mini-issuer` writes into its keybox:
/// `SecretKey::to_sec1_der` omits `parameters`, which OhMyKeymint refuses.
fn sec1_key_der(secret: &SecretKey) -> Vec<u8> {
    let private = Zeroizing::new(secret.to_bytes());
    let public = secret.public_key().to_encoded_point(false);
    sec1::EcPrivateKey {
        private_key: &private[..],
        parameters: Some(sec1::EcParameters::NamedCurve(SECP256R1)),
        public_key: Some(public.as_bytes()),
    }
    .to_der()
    .unwrap()
}

fn pem(label: &str, der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut body = String::new();
    for chunk in b64.as_bytes().chunks(64) {
        body.push_str(std::str::from_utf8(chunk).unwrap());
        body.push('\n');
    }
    format!("-----BEGIN {label}-----\n{body}-----END {label}-----\n")
}

/// Same layout as `rkp-mini`'s `keybox::build`.
pub fn keybox_xml(key_pem: &str, chain_pems: &[String], device_id: &str) -> String {
    let mut certs = String::new();
    for p in chain_pems {
        certs.push_str("                <Certificate format=\"pem\">\n");
        certs.push_str(p);
        certs.push_str("                </Certificate>\n");
    }
    format!(
        "<?xml version=\"1.0\"?>\n\
<AndroidAttestation>\n\
    <NumberOfKeyboxes>1</NumberOfKeyboxes>\n\
    <Keybox DeviceID=\"{device_id}\">\n\
        <Key algorithm=\"ecdsa\">\n\
            <PrivateKey format=\"pem\">\n\
{key_pem}\
            </PrivateKey>\n\
            <CertificateChain>\n\
                <NumberOfCertificates>{n}</NumberOfCertificates>\n\
{certs}\
            </CertificateChain>\n\
        </Key>\n\
    </Keybox>\n\
</AndroidAttestation>\n",
        n = chain_pems.len(),
    )
}

impl TestKeybox {
    /// Fresh CA + leaf; the leaf expires `leaf_days` from now.
    pub fn new(leaf_days: i64) -> TestKeybox {
        let now = OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.distinguished_name = dn("RKP Sync Test Root");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.not_before = now - Duration::days(1);
        ca_params.not_after = now + Duration::days(3650);
        let ca = ca_params.self_signed(&ca_key).unwrap();

        let secret = SecretKey::random(&mut OsRng);
        let leaf_key = KeyPair::try_from(secret.to_pkcs8_der().unwrap().as_bytes()).unwrap();
        let mut leaf_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        leaf_params.distinguished_name = dn("RKP Sync Test Leaf");
        leaf_params.not_before = now - Duration::days(1);
        leaf_params.not_after = now + Duration::days(leaf_days);
        let leaf = leaf_params.signed_by(&leaf_key, &ca, &ca_key).unwrap();

        let leaf_der = leaf.der().as_ref().to_vec();
        let ca_der = ca.der().as_ref().to_vec();
        let chain_pems = vec![pem("CERTIFICATE", &leaf_der), pem("CERTIFICATE", &ca_der)];
        let mut chain_der = leaf_der;
        chain_der.extend_from_slice(&ca_der);

        let sec1 = secret.public_key().to_encoded_point(false);
        let device_id = format!("rkp-mini-{}", &crate::sha256_hex(sec1.as_bytes())[..12]);
        let key_pem = pem("EC PRIVATE KEY", &sec1_key_der(&secret));
        let xml = keybox_xml(&key_pem, &chain_pems, &device_id);
        TestKeybox {
            xml,
            key_pem,
            chain_pems,
            chain_der,
            device_id,
        }
    }
}
