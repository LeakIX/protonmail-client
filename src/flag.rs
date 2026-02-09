//! IMAP message flags
//!
//! Provides a strongly-typed enum for IMAP flags instead of raw
//! strings. Standard system flags have dedicated variants; arbitrary
//! keyword flags use the `Keyword` variant.

use std::fmt;

/// An IMAP message flag.
///
/// System flags (prefixed with `\` in the IMAP protocol) have
/// dedicated variants. User-defined keyword flags use [`Flag::Keyword`].
///
/// # Examples
///
/// ```
/// use protonmail_client::Flag;
///
/// let seen = Flag::Seen;
/// assert_eq!(seen.as_imap_str(), "\\Seen");
///
/// let kw = Flag::Keyword("$Important".to_string());
/// assert_eq!(kw.as_imap_str(), "$Important");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Flag {
    /// Message has been read (`\Seen`).
    Seen,
    /// Message has been answered (`\Answered`).
    Answered,
    /// Message is flagged for attention (`\Flagged`).
    Flagged,
    /// Message is marked for deletion (`\Deleted`).
    Deleted,
    /// Message is a draft (`\Draft`).
    Draft,
    /// A user-defined keyword flag (no `\` prefix).
    Keyword(String),
}

impl Flag {
    /// The IMAP wire representation of this flag.
    ///
    /// System flags include the leading backslash (e.g. `\Seen`).
    /// Keyword flags are returned as-is.
    #[must_use]
    pub fn as_imap_str(&self) -> &str {
        match self {
            Self::Seen => "\\Seen",
            Self::Answered => "\\Answered",
            Self::Flagged => "\\Flagged",
            Self::Deleted => "\\Deleted",
            Self::Draft => "\\Draft",
            Self::Keyword(kw) => kw,
        }
    }
}

impl fmt::Display for Flag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_imap_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_flags() {
        assert_eq!(Flag::Seen.as_imap_str(), "\\Seen");
        assert_eq!(Flag::Answered.as_imap_str(), "\\Answered");
        assert_eq!(Flag::Flagged.as_imap_str(), "\\Flagged");
        assert_eq!(Flag::Deleted.as_imap_str(), "\\Deleted");
        assert_eq!(Flag::Draft.as_imap_str(), "\\Draft");
    }

    #[test]
    fn keyword_flag() {
        let kw = Flag::Keyword("$Important".to_string());
        assert_eq!(kw.as_imap_str(), "$Important");
    }

    #[test]
    fn display_matches_imap_str() {
        assert_eq!(format!("{}", Flag::Seen), "\\Seen");
        assert_eq!(format!("{}", Flag::Keyword("$Junk".to_string())), "$Junk");
    }
}
