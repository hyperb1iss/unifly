use std::path::Path;
use std::sync::Arc;

use rustls::ClientConfig;
use rustls_pki_types::CertificateDer;
use tokio_tungstenite::Connector;

use crate::error::Error;
use crate::transport::TlsMode;

pub(in crate::websocket) fn build_tls_connector(
    tls_mode: &TlsMode,
) -> Result<Option<Connector>, Error> {
    crate::transport::ensure_crypto_provider();

    match tls_mode {
        TlsMode::System => Ok(None),
        TlsMode::CustomCa(path) => {
            let root_store = load_root_store(path)?;
            let tls_config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
            Ok(Some(Connector::Rustls(Arc::new(tls_config))))
        }
        TlsMode::DangerAcceptInvalid => {
            let tls_config = ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerifier))
                .with_no_client_auth();
            Ok(Some(Connector::Rustls(Arc::new(tls_config))))
        }
    }
}

fn load_root_store(path: &Path) -> Result<rustls::RootCertStore, Error> {
    use rustls_pki_types::pem::PemObject;

    let mut root_store = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_file_iter(path)
        .map_err(|error| Error::Tls(format!("failed to read CA cert: {error}")))?
    {
        let cert = cert.map_err(|error| Error::Tls(format!("invalid PEM in CA file: {error}")))?;
        root_store
            .add(cert)
            .map_err(|error| Error::Tls(format!("invalid CA cert: {error}")))?;
    }
    Ok(root_store)
}

#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
