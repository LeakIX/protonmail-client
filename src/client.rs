//! Proton Mail IMAP client with typestate access control
//!
//! `ProtonClient<M>` is parameterised by an access mode:
//!
//! - [`ReadOnly`]  -- only read operations (list, fetch, search)
//! - [`ReadWrite`] -- read **and** write operations (move, flag,
//!   archive)
//!
//! This prevents accidental use of destructive operations when only
//! read access is intended.
//!
//! ```rust,no_run
//! use protonmail_client::{ImapConfig, ProtonClient, ReadOnly, ReadWrite};
//!
//! let cfg = ImapConfig::from_env().unwrap();
//!
//! // Read-only client -- write methods are not available.
//! let reader: ProtonClient<ReadOnly> = ProtonClient::new(cfg.clone());
//!
//! // Read-write client -- all methods are available.
//! let writer: ProtonClient<ReadWrite> = ProtonClient::new(cfg);
//! ```
//!
//! A read-only client cannot call write operations — this fails to
//! compile:
//!
//! ```compile_fail
//! use protonmail_client::{Flag, Folder, ImapConfig, ProtonClient};
//!
//! let cfg = ImapConfig::from_env().unwrap();
//! let client: ProtonClient = ProtonClient::new(cfg);
//! // ERROR: no method named `add_flag` found for `ProtonClient<ReadOnly>`
//! let _ = client.add_flag(1, &Folder::Inbox, &Flag::Seen);
//! ```
//!
//! ```compile_fail
//! use protonmail_client::{Folder, ImapConfig, ProtonClient};
//!
//! let cfg = ImapConfig::from_env().unwrap();
//! let client: ProtonClient = ProtonClient::new(cfg);
//! // ERROR: no method named `move_to_folder` found for `ProtonClient<ReadOnly>`
//! let _ = client.move_to_folder(1, &Folder::Inbox, &Folder::Trash);
//! ```
//!
//! ```compile_fail
//! use protonmail_client::{Folder, ImapConfig, ProtonClient};
//!
//! let cfg = ImapConfig::from_env().unwrap();
//! let client: ProtonClient = ProtonClient::new(cfg);
//! // ERROR: no method named `archive` found for `ProtonClient<ReadOnly>`
//! let _ = client.archive(1, &Folder::Inbox);
//! ```

use std::marker::PhantomData;

use crate::config::ImapConfig;
use crate::connection::{self, ImapSession};
use crate::error::{Error, Result};
use crate::flag::Flag;
use crate::folder::Folder;
use chrono::NaiveDate;
use email_extract::{Email, parse_email};
use futures::{StreamExt, pin_mut};
use tracing::{info, warn};

// ── Access-mode markers ────────────────────────────────────────────

/// Marker: read-only access. Write methods are not available.
#[derive(Debug, Clone, Copy)]
pub struct ReadOnly;

/// Marker: read-write access. All methods are available.
#[derive(Debug, Clone, Copy)]
pub struct ReadWrite;

// ── Client ─────────────────────────────────────────────────────────

/// IMAP client for Proton Mail via Proton Bridge.
///
/// The type parameter `M` controls which operations are available:
///
/// | `M`         | Read ops | Write ops |
/// |-------------|----------|-----------|
/// | `ReadOnly`  | yes      | no        |
/// | `ReadWrite` | yes      | yes       |
pub struct ProtonClient<M = ReadOnly> {
    config: ImapConfig,
    _mode: PhantomData<M>,
}

impl<M> ProtonClient<M> {
    #[must_use]
    pub const fn new(config: ImapConfig) -> Self {
        Self {
            config,
            _mode: PhantomData,
        }
    }
}

// ── Read operations (available on any M) ───────────────────────────

impl<M: Send + Sync> ProtonClient<M> {
    /// List all available IMAP folders.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection or LIST command fails.
    pub async fn list_folders(&self) -> Result<Vec<String>> {
        let mut session = connection::connect(&self.config).await?;

        let mut folder_stream = session
            .list(Some(""), Some("*"))
            .await
            .map_err(|e| Error::Imap(format!("List folders failed: {e}")))?;

        let mut names = Vec::new();
        while let Some(item) = folder_stream.next().await {
            if let Ok(name) = item {
                names.push(name.name().to_string());
            }
        }
        drop(folder_stream);

        session.logout().await.ok();
        Ok(names)
    }

    /// Fetch a single email by UID from a folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or FETCH fails,
    /// or if the message body cannot be parsed.
    pub async fn fetch_uid(&self, folder: &Folder, uid: u32) -> Result<Email> {
        let mut session = connection::connect(&self.config).await?;
        connection::select(&mut session, folder.as_str()).await?;

        let email = Self::fetch_single(&mut session, uid).await?;

        session.logout().await.ok();
        Ok(email)
    }

    /// Fetch all unseen emails from a folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn fetch_unseen(&self, folder: &Folder) -> Result<Vec<Email>> {
        self.search(folder, "UNSEEN").await
    }

    /// Fetch all emails from a folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn fetch_all(&self, folder: &Folder) -> Result<Vec<Email>> {
        self.search(folder, "ALL").await
    }

    /// Fetch the N most recent emails from a folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, SEARCH, or
    /// FETCH fails.
    pub async fn fetch_last_n(&self, folder: &Folder, n: usize) -> Result<Vec<Email>> {
        let mut session = connection::connect(&self.config).await?;
        connection::select(&mut session, folder.as_str()).await?;

        let uids = session
            .uid_search("ALL")
            .await
            .map_err(|e| Error::Imap(format!("Search failed: {e}")))?;

        let mut uid_list: Vec<u32> = uids.into_iter().collect();
        uid_list.sort_unstable();

        let start = uid_list.len().saturating_sub(n);
        let recent_uids = &uid_list[start..];

        if recent_uids.is_empty() {
            session.logout().await.ok();
            return Ok(vec![]);
        }

        info!("Fetching {} most recent messages", recent_uids.len());

        let mut emails = Self::fetch_by_uids(&mut session, recent_uids).await?;
        emails.sort_by(|a, b| b.date.cmp(&a.date));

        session.logout().await.ok();
        Ok(emails)
    }

    /// Fetch emails within a date range from a folder.
    ///
    /// IMAP semantics: SINCE >= date, BEFORE < date.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn fetch_date_range(
        &self,
        folder: &Folder,
        since: NaiveDate,
        before: NaiveDate,
    ) -> Result<Vec<Email>> {
        let since_str = since.format("%-d-%b-%Y").to_string();
        let before_str = before.format("%-d-%b-%Y").to_string();
        let query = format!("SINCE {since_str} BEFORE {before_str}");

        let mut emails = self.search(folder, &query).await?;
        emails.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(emails)
    }

    /// Search emails using an arbitrary IMAP search query.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or SEARCH fails.
    pub async fn search(&self, folder: &Folder, query: &str) -> Result<Vec<Email>> {
        let mut session = connection::connect(&self.config).await?;
        connection::select(&mut session, folder.as_str()).await?;

        let uids = session
            .uid_search(query)
            .await
            .map_err(|e| Error::Imap(format!("Search failed: {e}")))?;

        let uid_list: Vec<u32> = uids.into_iter().collect();
        if uid_list.is_empty() {
            session.logout().await.ok();
            return Ok(vec![]);
        }

        info!("Found {} messages matching '{}'", uid_list.len(), query);

        let emails = Self::fetch_by_uids(&mut session, &uid_list).await?;

        session.logout().await.ok();
        Ok(emails)
    }

    // -- private helpers (read) --

    async fn fetch_by_uids(session: &mut ImapSession, uids: &[u32]) -> Result<Vec<Email>> {
        let mut emails = Vec::new();

        for uid in uids {
            match Self::fetch_single(session, *uid).await {
                Ok(email) => emails.push(email),
                Err(e) => {
                    warn!("Failed to fetch UID {}: {}", uid, e);
                }
            }
        }

        Ok(emails)
    }

    async fn fetch_single(session: &mut ImapSession, uid: u32) -> Result<Email> {
        let uid_set = format!("{uid}");
        let mut messages = session
            .uid_fetch(&uid_set, "(BODY.PEEK[])")
            .await
            .map_err(|e| Error::Imap(format!("Fetch failed: {e}")))?;

        if let Some(msg_result) = messages.next().await {
            let msg = msg_result.map_err(|e| Error::Imap(format!("Fetch error: {e}")))?;
            if let Some(body) = msg.body() {
                return parse_email(uid, body).map_err(|e| Error::Parse(e.to_string()));
            }
        }

        Err(Error::Imap(format!("No body found for UID {uid}")))
    }
}

// ── Write operations (only on ReadWrite) ───────────────────────────

impl ProtonClient<ReadWrite> {
    /// Move an email from one folder to another.
    ///
    /// Selects `from`, copies the message to `to`, marks it
    /// `\Deleted` in the source folder, and expunges.
    ///
    /// # Errors
    ///
    /// Returns an error if any IMAP command fails.
    pub async fn move_to_folder(&self, uid: u32, from: &Folder, to: &Folder) -> Result<()> {
        let mut session = connection::connect(&self.config).await?;
        connection::select(&mut session, from.as_str()).await?;

        let uid_set = format!("{uid}");

        // COPY to destination
        session
            .uid_copy(&uid_set, to.as_str())
            .await
            .map_err(|e| Error::Imap(format!("Copy failed: {e}")))?;

        // Mark \Deleted in source
        let mut store_stream = session
            .uid_store(&uid_set, "+FLAGS (\\Deleted)")
            .await
            .map_err(|e| Error::Imap(format!("Store +Deleted failed: {e}")))?;
        while store_stream.next().await.is_some() {}
        drop(store_stream);

        // Expunge to permanently remove
        {
            let expunge_stream = session
                .expunge()
                .await
                .map_err(|e| Error::Imap(format!("Expunge failed: {e}")))?;
            pin_mut!(expunge_stream);
            while expunge_stream.next().await.is_some() {}
        }

        session.logout().await.ok();
        Ok(())
    }

    /// Add a flag to an email.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or STORE fails.
    pub async fn add_flag(&self, uid: u32, folder: &Folder, flag: &Flag) -> Result<()> {
        let mut session = connection::connect(&self.config).await?;
        connection::select(&mut session, folder.as_str()).await?;

        let uid_set = format!("{uid}");
        let store_arg = format!("+FLAGS ({})", flag.as_imap_str());

        let mut stream = session
            .uid_store(&uid_set, &store_arg)
            .await
            .map_err(|e| Error::Imap(format!("Store failed: {e}")))?;
        while stream.next().await.is_some() {}
        drop(stream);

        session.logout().await.ok();
        Ok(())
    }

    /// Remove a flag from an email.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, or STORE fails.
    pub async fn remove_flag(&self, uid: u32, folder: &Folder, flag: &Flag) -> Result<()> {
        let mut session = connection::connect(&self.config).await?;
        connection::select(&mut session, folder.as_str()).await?;

        let uid_set = format!("{uid}");
        let store_arg = format!("-FLAGS ({})", flag.as_imap_str());

        let mut stream = session
            .uid_store(&uid_set, &store_arg)
            .await
            .map_err(|e| Error::Imap(format!("Store failed: {e}")))?;
        while stream.next().await.is_some() {}
        drop(stream);

        session.logout().await.ok();
        Ok(())
    }

    /// Archive an email by moving it to the Archive folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the move operation fails.
    pub async fn archive(&self, uid: u32, from: &Folder) -> Result<()> {
        self.move_to_folder(uid, from, &Folder::Archive).await
    }

    /// Remove the `\Seen` flag from all messages in a folder.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection, SELECT, SEARCH, or STORE
    /// fails.
    pub async fn unmark_all_read(&self, folder: &Folder) -> Result<()> {
        let mut session = connection::connect(&self.config).await?;
        connection::select(&mut session, folder.as_str()).await?;

        let uids = session
            .uid_search("SEEN")
            .await
            .map_err(|e| Error::Imap(format!("Search failed: {e}")))?;

        let uid_list: Vec<u32> = uids.into_iter().collect();
        if uid_list.is_empty() {
            session.logout().await.ok();
            return Ok(());
        }

        let uid_set = uid_list
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");

        let mut stream = session
            .uid_store(&uid_set, "-FLAGS (\\Seen)")
            .await
            .map_err(|e| Error::Imap(format!("Store failed: {e}")))?;
        while stream.next().await.is_some() {}
        drop(stream);

        session.logout().await.ok();
        Ok(())
    }
}
