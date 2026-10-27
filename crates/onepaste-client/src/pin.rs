//! Certificate pinning: the connection is accepted only if the server certificate's SPKI fingerprint matches the one in
//! the connection code. The CA chain and the host name are ignored, but the handshake signature is always verified.

use onepaste_core::pin::fingerprint_of_cert_der;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::sync::Arc;

#[derive(Debug)]
pub struct PinVerifier {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl PinVerifier {
    pub fn new(fingerprint: &str) -> PinVerifier {
        PinVerifier { fingerprint: fingerprint.to_string(), provider: Arc::new(ring::default_provider()) }
    }
}

/// Constant-time comparison of two fingerprints.
fn same(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let got = fingerprint_of_cert_der(end_entity.as_ref()).unwrap_or_default();
        if same(&got, &self.fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("certificate fingerprint mismatch".into()))
        }
    }

    // The signature checks are delegated to the crypto provider and never skipped: without them anyone holding the
    // (public) certificate could impersonate the server.
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// An HTTPS client that trusts exactly one certificate, identified by its fingerprint.
pub fn pinned_client(fingerprint: &str) -> reqwest::Client {
    let provider = Arc::new(ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier::new(fingerprint)))
        .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(cfg)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("the client configuration is valid")
}
