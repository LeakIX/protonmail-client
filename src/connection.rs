//! Shared IMAP connection and TLS helpers
//!
//! Provides the low-level `connect()` and `select()` functions used by
//! both read and write operations on `ProtonClient`.

use crate::config::ImapConfig;
use crate::error::{Error, Result};
use async_imap::Session;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use tracing::{debug, info};

/// A TLS-wrapped IMAP session.
pub type ImapSession = Session<Compat<tokio_rustls::client::TlsStream<TcpStream>>>;

/// Build a TLS connector that accepts all certificates.
///
/// Proton Bridge uses self-signed certificates, so we skip
/// verification entirely.
fn tls_connector() -> TlsConnector {
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DangerousVerifier))
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

/// Open a fresh TLS-wrapped IMAP session.
///
/// Connects to `config.host:config.port` via TCP, issues STARTTLS,
/// performs the TLS handshake, and logs in.
pub async fn connect(config: &ImapConfig) -> Result<ImapSession> {
    let addr = format!("{}:{}", config.host, config.port);
    debug!("Connecting to IMAP server at {}", addr);

    let tcp_stream = TcpStream::connect(&addr).await?;
    let mut client = async_imap::Client::new(tcp_stream.compat());

    client
        .run_command_and_check_ok("STARTTLS", None)
        .await
        .map_err(|e| Error::Tls(format!("STARTTLS failed: {e}")))?;

    let connector = tls_connector();
    let server_name = ServerName::try_from(config.host.clone())
        .map_err(|e| Error::Tls(format!("Invalid server name: {e}")))?;

    let inner = client.into_inner().into_inner();
    let tls_stream = connector
        .connect(server_name, inner)
        .await
        .map_err(|e| Error::Tls(e.to_string()))?;

    let tls_client = async_imap::Client::new(tls_stream.compat());

    let session = tls_client
        .login(&config.username, &config.password)
        .await
        .map_err(|(e, _)| Error::Imap(format!("Login failed: {e}")))?;

    info!("Connected to IMAP server");
    Ok(session)
}

/// SELECT a folder on an existing session.
pub async fn select(session: &mut ImapSession, folder: &str) -> Result<()> {
    session
        .select(folder)
        .await
        .map_err(|e| Error::Imap(format!("Failed to select {folder}: {e}")))?;
    Ok(())
}

/// Certificate verifier that accepts all certificates
/// (for Proton Bridge self-signed certs).
#[derive(Debug)]
struct DangerousVerifier;

impl rustls::client::danger::ServerCertVerifier for DangerousVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
