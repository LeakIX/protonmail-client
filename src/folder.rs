//! IMAP folder types
//!
//! Provides a strongly-typed enum for IMAP folders instead of raw
//! strings. Well-known folders like INBOX, Sent, and Trash have
//! dedicated constructors. User-defined folders use the `Custom`
//! variant.

use std::fmt;

/// An IMAP mailbox folder.
///
/// Well-known folders have dedicated variants that map to their
/// standard IMAP names. For user-created folders, use
/// [`Folder::custom`].
///
/// # Examples
///
/// ```
/// use protonmail_client::Folder;
///
/// let inbox = Folder::Inbox;
/// assert_eq!(inbox.as_str(), "INBOX");
///
/// let custom = Folder::custom("My Projects");
/// assert_eq!(custom.as_str(), "My Projects");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Folder {
    /// The INBOX folder (RFC 3501 required, case-insensitive).
    Inbox,
    /// Sent messages.
    Sent,
    /// Draft messages.
    Drafts,
    /// Deleted messages.
    Trash,
    /// Spam / junk messages.
    Spam,
    /// Archived messages.
    Archive,
    /// A user-defined or server-specific folder.
    Custom(String),
}

impl Folder {
    /// Create a folder for a user-defined or non-standard mailbox.
    #[must_use]
    pub fn custom(name: impl Into<String>) -> Self {
        Self::Custom(name.into())
    }

    /// The IMAP folder name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Inbox => "INBOX",
            Self::Sent => "Sent",
            Self::Drafts => "Drafts",
            Self::Trash => "Trash",
            Self::Spam => "Spam",
            Self::Archive => "Archive",
            Self::Custom(name) => name,
        }
    }
}

impl fmt::Display for Folder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Folder {
    fn from(s: &str) -> Self {
        if s.eq_ignore_ascii_case("inbox") {
            Self::Inbox
        } else {
            match s {
                "Sent" => Self::Sent,
                "Drafts" => Self::Drafts,
                "Trash" => Self::Trash,
                "Spam" => Self::Spam,
                "Archive" => Self::Archive,
                other => Self::Custom(other.to_string()),
            }
        }
    }
}

impl From<String> for Folder {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_name() {
        assert_eq!(Folder::Inbox.as_str(), "INBOX");
    }

    #[test]
    fn custom_name() {
        let f = Folder::custom("Work");
        assert_eq!(f.as_str(), "Work");
    }

    #[test]
    fn from_str_inbox_case_insensitive() {
        assert_eq!(Folder::from("inbox"), Folder::Inbox);
        assert_eq!(Folder::from("INBOX"), Folder::Inbox);
        assert_eq!(Folder::from("Inbox"), Folder::Inbox);
    }

    #[test]
    fn from_str_known_folders() {
        assert_eq!(Folder::from("Sent"), Folder::Sent);
        assert_eq!(Folder::from("Drafts"), Folder::Drafts);
        assert_eq!(Folder::from("Trash"), Folder::Trash);
        assert_eq!(Folder::from("Spam"), Folder::Spam);
        assert_eq!(Folder::from("Archive"), Folder::Archive);
    }

    #[test]
    fn from_str_unknown_becomes_custom() {
        assert_eq!(
            Folder::from("My Stuff"),
            Folder::Custom("My Stuff".to_string())
        );
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(format!("{}", Folder::Inbox), "INBOX");
        assert_eq!(format!("{}", Folder::custom("Notes")), "Notes");
    }
}
