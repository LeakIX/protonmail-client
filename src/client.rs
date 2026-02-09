//! Proton Mail IMAP client

use crate::config::ImapConfig;
use crate::error::{Error, Result};
use async_imap::Session;
use chrono::NaiveDate;
use email_parser::{Email, parse_email};
use futures::StreamExt;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use tracing::{debug, info, warn};

type ImapSession = Session<Compat<tokio_rustls::client::TlsStream<TcpStream>>>;

/// Read-only IMAP client for Proton Mail via Proton Bridge
pub struct ProtonClient {
    config: ImapConfig,
}

impl ProtonClient {
    #[must_use]
    pub const fn new(config: ImapConfig) -> Self {
        Self { config }
    }

    /// List all available IMAP folders
    ///
    /// # Errors
    ///
    /// Returns an error if the connection or LIST command fails.
    pub async fn list_folders(&self) -> Result<Vec<String>> {
        let mut session = self.connect().await?;

        let mut folder_stream = session
            .list(Some(""), Some("*"))
            .await
            .map_err(|e| Error::Imap(format!("List folders failed: {e}")))?;

        let mut names = Vec::new();
        while let Some(item) = folder_stream.next().await {
            if let Ok(name) = item {
                names.push(name.name().to_string());
            }
        }
        drop(folder_stream);

        session.logout().await.ok();
        Ok(names)
    }

    /// Fetch a single email by UID from a folder
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or FETCH fails,
    /// or if the message body cannot be parsed.
    pub async fn fetch_uid(&self, folder: &str, uid: u32) -> Result<Email> {
        let mut session = self.connect().await?;
        self.select(&mut session, folder).await?;

        let email = self.fetch_single(&mut session, uid).await?;

        session.logout().await.ok();
        Ok(email)
    }

    /// Fetch all unseen emails from a folder
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn fetch_unseen(&self, folder: &str) -> Result<Vec<Email>> {
        self.search(folder, "UNSEEN").await
    }

    /// Fetch all emails from a folder
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn fetch_all(&self, folder: &str) -> Result<Vec<Email>> {
        self.search(folder, "ALL").await
    }

    /// Fetch the N most recent emails from a folder
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, SEARCH, or
    /// FETCH fails.
    pub async fn fetch_last_n(&self, folder: &str, n: usize) -> Result<Vec<Email>> {
        let mut session = self.connect().await?;
        self.select(&mut session, folder).await?;

        let uids = session
            .uid_search("ALL")
            .await
            .map_err(|e| Error::Imap(format!("Search failed: {e}")))?;

        let mut uid_list: Vec<u32> = uids.into_iter().collect();
        uid_list.sort_unstable();

        let start = uid_list.len().saturating_sub(n);
        let recent_uids = &uid_list[start..];

        if recent_uids.is_empty() {
            session.logout().await.ok();
            return Ok(vec![]);
        }

        info!("Fetching {} most recent messages", recent_uids.len());

        let mut emails = self.fetch_by_uids(&mut session, recent_uids).await?;
        emails.sort_by(|a, b| b.date.cmp(&a.date));

        session.logout().await.ok();
        Ok(emails)
    }

    /// Fetch emails within a date range from a folder
    ///
    /// IMAP semantics: SINCE >= date, BEFORE < date.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn fetch_date_range(
        &self,
        folder: &str,
        since: NaiveDate,
        before: NaiveDate,
    ) -> Result<Vec<Email>> {
        let since_str = since.format("%-d-%b-%Y").to_string();
        let before_str = before.format("%-d-%b-%Y").to_string();
        let query = format!("SINCE {since_str} BEFORE {before_str}");

        let mut emails = self.search(folder, &query).await?;
        emails.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(emails)
    }

    /// Search emails using an arbitrary IMAP search query
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn search(&self, folder: &str, query: &str) -> Result<Vec<Email>> {
        let mut session = self.connect().await?;
        self.select(&mut session, folder).await?;

        let uids = session
            .uid_search(query)
            .await
            .map_err(|e| Error::Imap(format!("Search failed: {e}")))?;

        let uid_list: Vec<u32> = uids.into_iter().collect();
        if uid_list.is_empty() {
            session.logout().await.ok();
            return Ok(vec![]);
        }

        info!("Found {} messages matching '{}'", uid_list.len(), query);

        let emails = self.fetch_by_uids(&mut session, &uid_list).await?;

        session.logout().await.ok();
        Ok(emails)
    }

    // -- private helpers --

    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn tls_connector(&self) -> Result<TlsConnector> {
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(DangerousVerifier))
            .with_no_client_auth();
        Ok(TlsConnector::from(Arc::new(config)))
    }

    async fn connect(&self) -> Result<ImapSession> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        debug!("Connecting to IMAP server at {}", addr);

        let tcp_stream = TcpStream::connect(&addr).await?;
        let mut client = async_imap::Client::new(tcp_stream.compat());

        client
            .run_command_and_check_ok("STARTTLS", None)
            .await
            .map_err(|e| Error::Tls(format!("STARTTLS failed: {e}")))?;

        let connector = self.tls_connector()?;
        let server_name = ServerName::try_from(self.config.host.clone())
            .map_err(|e| Error::Tls(format!("Invalid server name: {e}")))?;

        let inner = client.into_inner().into_inner();
        let tls_stream = connector
            .connect(server_name, inner)
            .await
            .map_err(|e| Error::Tls(e.to_string()))?;

        let tls_client = async_imap::Client::new(tls_stream.compat());

        let session = tls_client
            .login(&self.config.username, &self.config.password)
            .await
            .map_err(|(e, _)| Error::Imap(format!("Login failed: {e}")))?;

        info!("Connected to IMAP server");
        Ok(session)
    }

    async fn select(&self, session: &mut ImapSession, folder: &str) -> Result<()> {
        session
            .select(folder)
            .await
            .map_err(|e| Error::Imap(format!("Failed to select {folder}: {e}")))?;
        Ok(())
    }

    async fn fetch_by_uids(&self, session: &mut ImapSession, uids: &[u32]) -> Result<Vec<Email>> {
        let mut emails = Vec::new();

        for uid in uids {
            match self.fetch_single(session, *uid).await {
                Ok(email) => emails.push(email),
                Err(e) => {
                    warn!("Failed to fetch UID {}: {}", uid, e);
                }
            }
        }

        Ok(emails)
    }

    async fn fetch_single(&self, session: &mut ImapSession, uid: u32) -> Result<Email> {
        let uid_set = format!("{uid}");
        let mut messages = session
            .uid_fetch(&uid_set, "(BODY.PEEK[])")
            .await
            .map_err(|e| Error::Imap(format!("Fetch failed: {e}")))?;

        if let Some(msg_result) = messages.next().await {
            let msg = msg_result.map_err(|e| Error::Imap(format!("Fetch error: {e}")))?;
            if let Some(body) = msg.body() {
                return parse_email(uid, body).map_err(|e| Error::Parse(e.to_string()));
            }
        }

        Err(Error::Imap(format!("No body found for UID {uid}")))
    }
}

/// Certificate verifier that accepts all certificates
/// (for Proton Bridge self-signed certs)
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
