#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

//! Proton Mail IMAP client library
//!
//! A read-only IMAP client for Proton Mail via Proton Bridge.
//! Provides email listing, search, folder enumeration, and
//! individual message retrieval over STARTTLS.

mod client;
mod config;
mod error;

pub use client::ProtonClient;
pub use config::ImapConfig;
pub use email_parser::Email;
pub use error::{Error, Result};
