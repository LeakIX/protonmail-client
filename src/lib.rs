#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

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

pub use client::ProtonClient;
pub use config::ImapConfig;
pub use email_parser::Email;
pub use error::{Error, Result};
