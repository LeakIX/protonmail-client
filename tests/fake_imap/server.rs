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

use super::mailbox::Mailbox;
use rcgen::generate_simple_self_signed;
use rustls::pki_types::PrivatePkcs8KeyDer;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
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

        let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().clone());
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
    pub fn port(&self) -> u16 {
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
    let mut tls_reader = BufReader::new(tls_stream);
    let mut selected_folder: Option<String> = None;

    loop {
        let mut line = String::new();
        match tls_reader.read_line(&mut line).await {
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
            // LOGIN username password
            //
            // In real IMAP, credentials are sent in plaintext over the
            // (now encrypted) connection. We accept any credentials
            // since this is a test server.
            let resp = format!("{tag} OK LOGIN completed\r\n");
            if write_line(&mut tls_reader, &resp).await.is_err() {
                break;
            }
        } else if upper.starts_with("LIST") {
            // LIST reference mailbox-pattern
            //
            // Returns all folders matching the pattern. The format is:
            //   * LIST (flags) delimiter "folder-name"
            //
            // Flags like \HasNoChildren tell the client about the
            // folder's properties. The delimiter "/" is the hierarchy
            // separator (e.g. "INBOX/subfolder").
            handle_list(tag, mailbox, &mut tls_reader).await;
        } else if upper.starts_with("SELECT") {
            // SELECT folder-name
            //
            // Opens a folder for reading. The server responds with
            // metadata about the folder: how many messages exist,
            // the UIDVALIDITY value (used to detect folder recreation),
            // and flags.
            selected_folder = handle_select(tag, rest, mailbox, &mut tls_reader).await;
        } else if upper.starts_with("UID SEARCH") {
            // UID SEARCH criteria
            //
            // Searches the selected folder and returns matching UIDs.
            // Unlike plain SEARCH which returns sequence numbers, UID
            // SEARCH returns UIDs that are stable across sessions.
            handle_uid_search(
                tag,
                rest,
                mailbox,
                selected_folder.as_deref(),
                &mut tls_reader,
            )
            .await;
        } else if upper.starts_with("UID FETCH") {
            // UID FETCH uid-set (data-items)
            //
            // Fetches message data by UID. The response uses IMAP
            // literals for binary-safe transfer of message bodies.
            handle_uid_fetch(
                tag,
                rest,
                mailbox,
                selected_folder.as_deref(),
                &mut tls_reader,
            )
            .await;
        } else if upper.starts_with("LOGOUT") {
            // LOGOUT
            //
            // The server sends a BYE untagged response (indicating the
            // connection is ending) followed by the tagged OK.
            let _ = write_line(&mut tls_reader, "* BYE\r\n").await;
            let resp = format!("{tag} OK LOGOUT completed\r\n");
            let _ = write_line(&mut tls_reader, &resp).await;
            break;
        } else {
            // Unknown command -- respond with BAD.
            let resp = format!("{tag} BAD Unknown command\r\n");
            if write_line(&mut tls_reader, &resp).await.is_err() {
                break;
            }
        }
    }
}

/// Handle the LIST command.
///
/// Responds with one `* LIST` line per folder, followed by the
/// tagged OK. The format follows RFC 3501 Section 7.2.2:
///
/// ```text
/// * LIST (\HasNoChildren) "/" "INBOX"
/// * LIST (\HasNoChildren) "/" "Sent"
/// A0002 OK LIST completed
/// ```
async fn handle_list<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    mailbox: &Mailbox,
    stream: &mut BufReader<S>,
) {
    for folder in &mailbox.folders {
        let line = format!("* LIST (\\HasNoChildren) \"/\" \"{}\"\r\n", folder.name);
        if write_line(stream, &line).await.is_err() {
            return;
        }
    }
    let resp = format!("{tag} OK LIST completed\r\n");
    let _ = write_line(stream, &resp).await;
}

/// Handle the SELECT command.
///
/// Opens a folder and responds with metadata. The key pieces are:
///
/// - `* N EXISTS` -- total number of messages in the folder.
/// - `* OK [UIDVALIDITY V]` -- a value that changes if the folder's
///   UID space is reset (e.g. the folder was deleted and recreated).
///   Clients use this to invalidate their UID caches.
///
/// Returns the selected folder name (or None if not found).
async fn handle_select<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    rest: &str,
    mailbox: &Mailbox,
    stream: &mut BufReader<S>,
) -> Option<String> {
    // Extract folder name: "SELECT INBOX" or "SELECT \"INBOX\""
    let folder_name = rest.splitn(2, ' ').nth(1).unwrap_or("").trim_matches('"');

    if let Some(folder) = mailbox.get_folder(folder_name) {
        let exists = format!("* {} EXISTS\r\n", folder.emails.len());
        let _ = write_line(stream, &exists).await;
        let _ = write_line(stream, "* OK [UIDVALIDITY 1]\r\n").await;
        let resp = format!("{tag} OK [READ-WRITE] SELECT completed\r\n");
        let _ = write_line(stream, &resp).await;
        Some(folder_name.to_string())
    } else {
        let resp = format!("{tag} NO Folder not found\r\n");
        let _ = write_line(stream, &resp).await;
        None
    }
}

/// Handle the UID SEARCH command.
///
/// Parses the search criteria and returns matching UIDs. We support:
///
/// - `ALL` -- returns every UID in the selected folder
/// - `UNSEEN` -- returns UIDs without the `\Seen` flag
/// - `SINCE <date> BEFORE <date>` -- we return all UIDs (date
///   filtering would require parsing the email Date header, which
///   is overkill for our tests)
///
/// The response format (RFC 3501 Section 7.2.5):
/// ```text
/// * SEARCH 1 2 3
/// A0003 OK SEARCH completed
/// ```
async fn handle_uid_search<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    rest: &str,
    mailbox: &Mailbox,
    selected_folder: Option<&str>,
    stream: &mut BufReader<S>,
) {
    let Some(folder_name) = selected_folder else {
        let resp = format!("{tag} BAD No folder selected\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    let Some(folder) = mailbox.get_folder(folder_name) else {
        let resp = format!("{tag} BAD Folder not found\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    // Extract the search criteria after "UID SEARCH ".
    let criteria = rest
        .strip_prefix("UID SEARCH ")
        .or_else(|| rest.strip_prefix("uid search "))
        .unwrap_or(rest)
        .to_uppercase();

    let uids: Vec<u32> = if criteria.contains("UNSEEN") {
        // UNSEEN: only emails without the \Seen flag
        folder
            .emails
            .iter()
            .filter(|e| !e.seen)
            .map(|e| e.uid)
            .collect()
    } else {
        // ALL, SINCE/BEFORE, or anything else: return all UIDs.
        // Real IMAP servers would parse dates, but for testing
        // we just return everything.
        folder.emails.iter().map(|e| e.uid).collect()
    };

    // Format: "* SEARCH uid1 uid2 uid3\r\n"
    // If no results, still send "* SEARCH\r\n" (empty result set).
    let uid_str: Vec<String> = uids.iter().map(ToString::to_string).collect();
    let search_line = format!("* SEARCH {}\r\n", uid_str.join(" "));
    let _ = write_line(stream, &search_line).await;
    let resp = format!("{tag} OK SEARCH completed\r\n");
    let _ = write_line(stream, &resp).await;
}

/// Handle the UID FETCH command.
///
/// This is the most complex IMAP response because it uses **counted
/// literals** to transfer message bodies. The format is:
///
/// ```text
/// * <seq> FETCH (UID <uid> BODY[] {<length>}
/// <exactly length bytes of raw RFC 2822 message>
/// )
/// ```
///
/// The `{length}\r\n` is an IMAP literal marker. It tells the client:
/// "the next `length` bytes are raw data, not IMAP protocol text."
/// After reading those bytes, the client expects the closing `)`.
///
/// We use the sequence number equal to the UID for simplicity (in real
/// IMAP, sequence numbers are assigned per-session based on the order
/// messages appear in the folder).
async fn handle_uid_fetch<S: AsyncRead + AsyncWrite + Unpin>(
    tag: &str,
    rest: &str,
    mailbox: &Mailbox,
    selected_folder: Option<&str>,
    stream: &mut BufReader<S>,
) {
    let Some(folder_name) = selected_folder else {
        let resp = format!("{tag} BAD No folder selected\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    let Some(folder) = mailbox.get_folder(folder_name) else {
        let resp = format!("{tag} BAD Folder not found\r\n");
        let _ = write_line(stream, &resp).await;
        return;
    };

    // Parse the UID from "UID FETCH <uid> (BODY.PEEK[])".
    // The format from async-imap is: "UID FETCH 42 (BODY.PEEK[])"
    let parts: Vec<&str> = rest.split_whitespace().collect();
    // parts: ["UID", "FETCH", "42", "(BODY.PEEK[])"]
    let uid: u32 = if parts.len() >= 3 {
        parts[2].parse().unwrap_or(0)
    } else {
        0
    };

    if let Some(email) = folder.emails.iter().find(|e| e.uid == uid) {
        let body_len = email.raw.len();

        // Build the FETCH response with an IMAP literal.
        //
        // The literal `{N}\r\n` tells the client that the next N
        // bytes are raw data. After those bytes, we send `)\r\n`
        // to close the FETCH response.
        //
        // Real IMAP servers also return FLAGS, INTERNALDATE, etc.
        // async-imap only needs UID and BODY[] for our use case.
        let header = format!("* {uid} FETCH (UID {uid} BODY[] {{{body_len}}}\r\n");
        if write_line(stream, &header).await.is_err() {
            return;
        }

        // Write the raw email bytes. This is the literal data --
        // exactly body_len bytes, no escaping, no line ending
        // interpretation.
        if write_bytes(stream, &email.raw).await.is_err() {
            return;
        }

        // Close the FETCH response parenthesis.
        if write_line(stream, ")\r\n").await.is_err() {
            return;
        }
    }

    let resp = format!("{tag} OK FETCH completed\r\n");
    let _ = write_line(stream, &resp).await;
}

/// Write a string to the stream and flush.
async fn write_line<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    line: &str,
) -> std::io::Result<()> {
    stream.get_mut().write_all(line.as_bytes()).await?;
    stream.get_mut().flush().await
}

/// Write raw bytes to the stream and flush.
async fn write_bytes<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    data: &[u8],
) -> std::io::Result<()> {
    stream.get_mut().write_all(data).await?;
    stream.get_mut().flush().await
}
