//! Proton Mail IMAP client library
//!
//! An interface to interact with Proton Mail using
//! [Proton Bridge](https://proton.me/mail/bridge). Connects over
//! STARTTLS with self-signed certificate support.
//!
//! ## Access control
//!
//! `ProtonClient` uses a typestate pattern to separate read and write
//! operations at compile time:
//!
//! - `ProtonClient<ReadOnly>` -- list, fetch, search (default)
//! - `ProtonClient<ReadWrite>` -- all of the above **plus** move,
//!   flag, archive, and unmark
//!
//! Returns parsed [`Email`] structs from the [`email_extract`] crate.

mod client;
mod config;
mod connection;
mod error;
mod flag;
mod folder;

pub use client::{ProtonClient, ReadOnly, ReadWrite};
pub use config::ImapConfig;
pub use email_extract::Email;
pub use error::{Error, Result};
pub use flag::Flag;
pub use folder::Folder;
