//! Shared I/O helpers for the fake IMAP server.
//!
//! These are thin wrappers around `AsyncWriteExt` that flush after
//! every write. Real IMAP servers would batch writes for performance,
//! but flushing eagerly keeps the test server simple and deterministic.

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// Write a string to the stream and flush.
pub async fn write_line<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    line: &str,
) -> std::io::Result<()> {
    stream.get_mut().write_all(line.as_bytes()).await?;
    stream.get_mut().flush().await
}

/// Write raw bytes to the stream and flush.
pub async fn write_bytes<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    data: &[u8],
) -> std::io::Result<()> {
    stream.get_mut().write_all(data).await?;
    stream.get_mut().flush().await
}
