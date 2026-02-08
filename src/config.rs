//! IMAP connection configuration

use crate::error::{Error, Result};
use std::env;

/// IMAP connection configuration for Proton Bridge
#[derive(Debug, Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl ImapConfig {
    /// Load IMAP configuration from environment variables
    ///
    /// Reads from `.env` file if present. Required variables:
    /// - `IMAP_USERNAME`
    /// - `IMAP_PASSWORD`
    ///
    /// Optional (with defaults):
    /// - `IMAP_HOST` (default: `127.0.0.1`)
    /// - `IMAP_PORT` (default: `1143`)
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            host: env::var("IMAP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: env::var("IMAP_PORT")
                .unwrap_or_else(|_| "1143".to_string())
                .parse()
                .map_err(|e| Error::Config(format!("Invalid IMAP_PORT: {e}")))?,
            username: env::var("IMAP_USERNAME")
                .map_err(|_| Error::Config("IMAP_USERNAME not set".into()))?,
            password: env::var("IMAP_PASSWORD")
                .map_err(|_| Error::Config("IMAP_PASSWORD not set".into()))?,
        })
    }
}
