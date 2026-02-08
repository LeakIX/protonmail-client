//! Error types for protonmail-client

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IMAP error: {0}")]
    Imap(String),

    #[error("Email parsing error: {0}")]
    Parse(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TLS error: {0}")]
    Tls(String),
}

pub type Result<T> = std::result::Result<T, Error>;
