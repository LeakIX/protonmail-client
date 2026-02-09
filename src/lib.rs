//! Proton Mail IMAP client library
//!
//! An interface to fetch emails from Proton Mail using
//! [Proton Bridge](https://proton.me/mail/bridge). This is a
//! read-only IMAP client that connects over STARTTLS with
//! self-signed certificate support.
//!
//! Returns parsed [`Email`] structs from the
//! [`email_parser`] crate.

mod client;
mod config;
mod error;
mod folder;

pub use client::ProtonClient;
pub use config::ImapConfig;
pub use email_parser::Email;
pub use error::{Error, Result};
pub use folder::Folder;
