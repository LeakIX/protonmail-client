//! Fake IMAP server for integration testing
//!
//! This module provides an in-process IMAP server that speaks enough
//! of the protocol to test `ProtonClient` end-to-end:
//!
//! TCP -> greeting -> STARTTLS -> TLS handshake -> LOGIN -> commands -> LOGOUT
//!
//! ## Module layout
//!
//! - `server` -- TCP listener, TLS setup, and connection dispatch
//! - `handlers/` -- one file per IMAP command (LIST, SELECT, etc.)
//! - `mailbox` -- test data model (folders, emails, builder)
//! - `io` -- shared write helpers

mod handlers;
mod io;
pub mod mailbox;
mod server;

pub use mailbox::MailboxBuilder;
pub use server::FakeImapServer;
