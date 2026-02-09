//! In-process fake IMAP server for integration testing
//!
//! # How IMAP works (educational overview)
//!
//! IMAP (Internet Message Access Protocol, RFC 3501) is a text-based
//! protocol for accessing email on a remote server. Unlike POP3, IMAP
//! keeps messages on the server and supports folders, flags, and
//! server-side search.
//!
//! ## Connection lifecycle
//!
//! ```text
//!   Client connects via TCP
//!       |
//!   Server sends greeting: "* OK IMAP4rev1 ready\r\n"
//!       |
//!   Client sends STARTTLS to upgrade the connection
//!       |
//!   TLS handshake (after this, all traffic is encrypted)
//!       |
//!   Client sends LOGIN with username and password
//!       |
//!   Client issues commands: LIST, SELECT, SEARCH, FETCH, ...
//!       |
//!   Client sends LOGOUT
//! ```
//!
//! ## Command format
//!
//! Every client command starts with a **tag** -- an arbitrary string
//! the client chooses (async-imap uses `A0001`, `A0002`, etc.). The
//! server echoes this tag in its completion response so the client can
//! match responses to commands:
//!
//! ```text
//!   Client:  A0001 LOGIN user pass
//!   Server:  A0001 OK LOGIN completed
//! ```
//!
//! Lines prefixed with `*` are **untagged** responses -- data the
//! server sends before the final tagged OK/NO/BAD:
//!
//! ```text
//!   Client:  A0002 LIST "" "*"
//!   Server:  * LIST (\HasNoChildren) "/" "INBOX"
//!   Server:  * LIST (\HasNoChildren) "/" "Sent"
//!   Server:  A0002 OK LIST completed
//! ```
//!
//! ## FETCH and literals
//!
//! The most interesting part of IMAP is how it transfers message
//! bodies. Since emails can contain arbitrary binary data, IMAP uses
//! **counted literals**: `{bytecount}\r\n` followed by exactly that
//! many raw bytes:
//!
//! ```text
//!   * 1 FETCH (UID 42 BODY[] {1234}
//!   <exactly 1234 bytes of raw RFC 2822 message>
//!   )
//! ```
//!
//! This is how async-imap knows when the message body ends -- it reads
//! exactly `bytecount` bytes, then expects the closing `)`.

use super::handlers::{
    handle_list, handle_login, handle_logout, handle_select, handle_uid_fetch, handle_uid_search,
};
use super::io::write_line;
use super::mailbox::Mailbox;
use rcgen::generate_simple_self_signed;
use rustls::pki_types::PrivatePkcs8KeyDer;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// A fake IMAP server that runs on localhost with an OS-assigned port.
///
/// The server generates a self-signed TLS certificate at startup using
/// `rcgen`, so no cert files are needed. It speaks enough of the IMAP
/// protocol to exercise `ProtonClient`'s full connection lifecycle:
/// greeting -> STARTTLS -> TLS -> LOGIN -> commands -> LOGOUT.
pub struct FakeImapServer {
    port: u16,
    /// Handle to the background task so it lives as long as the server.
    _handle: tokio::task::JoinHandle<()>,
}

impl FakeImapServer {
    /// Start a new fake IMAP server with the given mailbox state.
    ///
    /// 1. Binds to `127.0.0.1:0` -- the OS picks a free port.
    /// 2. Generates a self-signed TLS certificate via `rcgen`.
    /// 3. Spawns a tokio task that accepts connections and speaks IMAP.
    ///
    /// The server runs until the `FakeImapServer` is dropped (the
    /// tokio task is aborted).
    pub async fn start(mailbox: Mailbox) -> Self {
        // Ensure the ring crypto provider is installed process-wide.
        // Multiple tests may race to install it, so we ignore the
        // error if it's already set.
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Bind to any available port on localhost.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to ephemeral port");
        let port = listener.local_addr().unwrap().port();

        // Generate a self-signed certificate. We use "127.0.0.1" as
        // the subject alt name since that's what the client connects
        // to. The client uses DangerousVerifier anyway, so the cert
        // details don't matter much -- but this is how you'd do it
        // properly with rcgen.
        let cert = generate_simple_self_signed(vec!["127.0.0.1".to_string()])
            .expect("generate self-signed cert");

        let cert_der = cert.cert.der().clone();
        let key_der = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());

        // Build a rustls ServerConfig with the generated cert.
        // This is the server-side counterpart to the client's
        // TlsConnector -- it presents the cert during the TLS
        // handshake.
        let tls_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der.into())
            .expect("build server TLS config");

        let acceptor = TlsAcceptor::from(Arc::new(tls_config));
        let mailbox = Arc::new(mailbox);

        // Spawn the accept loop. Each incoming connection gets its own
        // task that runs the IMAP state machine.
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _addr)) = listener.accept().await else {
                    break;
                };
                let acceptor = acceptor.clone();
                let mailbox = mailbox.clone();
                tokio::spawn(async move {
                    handle_connection(stream, acceptor, &mailbox).await;
                });
            }
        });

        Self {
            port,
            _handle: handle,
        }
    }

    /// The port the server is listening on.
    pub const fn port(&self) -> u16 {
        self.port
    }
}

/// Handle a single IMAP client connection.
///
/// This function implements the full IMAP lifecycle:
/// 1. Send the server greeting (pre-TLS, on the raw TCP stream)
/// 2. Wait for the STARTTLS command and upgrade to TLS
/// 3. Process authenticated commands (LOGIN, LIST, SELECT, etc.)
async fn handle_connection(
    stream: tokio::net::TcpStream,
    acceptor: TlsAcceptor,
    mailbox: &Mailbox,
) {
    // Phase 1: Pre-TLS communication
    //
    // The IMAP greeting is sent in plaintext. This matches real IMAP
    // servers that advertise STARTTLS capability. The client reads
    // this greeting, then sends the STARTTLS command to upgrade.
    let mut reader = BufReader::new(stream);

    // RFC 3501 Section 7.1.1: Server greeting
    // "* OK" means the server is ready and not pre-authenticated.
    if write_line(&mut reader, "* OK IMAP4rev1 Fake server ready\r\n")
        .await
        .is_err()
    {
        return;
    }

    // Read the STARTTLS command. In theory the client could send other
    // commands first (like CAPABILITY), but ProtonClient always starts
    // with STARTTLS immediately.
    let mut line = String::new();
    if reader.read_line(&mut line).await.is_err() {
        return;
    }

    // Parse the tag and command. The line looks like "A0001 STARTTLS\r\n".
    let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
    if parts.len() < 2 {
        return;
    }
    let tag = parts[0];
    let command = parts[1].to_uppercase();

    if command != "STARTTLS" {
        // We only accept STARTTLS as the first command.
        let resp = format!("{tag} BAD Expected STARTTLS\r\n");
        let _ = write_line(&mut reader, &resp).await;
        return;
    }

    // Tell the client to begin the TLS handshake. After this response,
    // the very next bytes on the wire are the TLS ClientHello.
    let resp = format!("{tag} OK Begin TLS negotiation now\r\n");
    if write_line(&mut reader, &resp).await.is_err() {
        return;
    }

    // Phase 2: TLS upgrade
    //
    // We recover the raw TcpStream from the BufReader, then hand it
    // to the TLS acceptor. This is the server-side TLS handshake --
    // the acceptor presents our self-signed cert and negotiates
    // encryption.
    let tcp = reader.into_inner();
    let Ok(tls_stream) = acceptor.accept(tcp).await else {
        return;
    };

    // Phase 3: Authenticated IMAP session
    //
    // After TLS is established, all communication is encrypted. The
    // client will now send LOGIN and then the real commands.
    handle_imap_session(tls_stream, mailbox).await;
}

/// Run the authenticated IMAP command loop over an established stream.
///
/// Dispatches each client command to the appropriate handler in
/// `handlers/`.
async fn handle_imap_session<S: AsyncRead + AsyncWrite + Unpin>(stream: S, mailbox: &Mailbox) {
    let mut reader = BufReader::new(stream);
    let mut selected_folder: Option<String> = None;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break, // Connection closed or error
            Ok(_) => {}
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Split into tag and the rest of the command.
        // Example: "A0003 UID SEARCH UNSEEN" -> tag="A0003",
        //   rest="UID SEARCH UNSEEN"
        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        if parts.len() < 2 {
            continue;
        }
        let tag = parts[0];
        let rest = parts[1];

        // Dispatch based on the command keyword. We match
        // case-insensitively since RFC 3501 says commands are
        // case-insensitive.
        let upper = rest.to_uppercase();

        if upper.starts_with("LOGIN") {
            if !handle_login(tag, &mut reader).await {
                break;
            }
        } else if upper.starts_with("LIST") {
            handle_list(tag, mailbox, &mut reader).await;
        } else if upper.starts_with("SELECT") {
            selected_folder = handle_select(tag, rest, mailbox, &mut reader).await;
        } else if upper.starts_with("UID SEARCH") {
            handle_uid_search(tag, rest, mailbox, selected_folder.as_deref(), &mut reader).await;
        } else if upper.starts_with("UID FETCH") {
            handle_uid_fetch(tag, rest, mailbox, selected_folder.as_deref(), &mut reader).await;
        } else if upper.starts_with("LOGOUT") {
            handle_logout(tag, &mut reader).await;
            break;
        } else {
            // Unknown command -- respond with BAD.
            let resp = format!("{tag} BAD Unknown command\r\n");
            if write_line(&mut reader, &resp).await.is_err() {
                break;
            }
        }
    }
}
