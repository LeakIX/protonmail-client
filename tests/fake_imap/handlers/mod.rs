//! IMAP command handlers for the fake server.
//!
//! Each handler lives in its own module and processes a single IMAP
//! command (LIST, SELECT, UID SEARCH, UID FETCH, LOGIN, LOGOUT).

mod list;
mod login;
mod logout;
mod select;
mod uid_fetch;
mod uid_search;

pub use list::handle_list;
pub use login::handle_login;
pub use logout::handle_logout;
pub use select::handle_select;
pub use uid_fetch::handle_uid_fetch;
pub use uid_search::handle_uid_search;
