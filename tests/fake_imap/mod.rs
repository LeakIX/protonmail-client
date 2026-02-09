//! Fake IMAP server for integration testing
//!
//! This module provides an in-process IMAP server that speaks enough
//! of the protocol to test `ProtonClient` end-to-end:
//!
//! TCP -> greeting -> STARTTLS -> TLS handshake -> LOGIN -> commands -> LOGOUT
//!
//! See `server.rs` for the protocol implementation with educational
//! comments, and `mailbox.rs` for the test data model.

mod mailbox;
mod server;

pub use mailbox::MailboxBuilder;
pub use server::FakeImapServer;
