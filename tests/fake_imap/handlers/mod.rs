//! IMAP command handlers for the fake server.
//!
//! Each handler lives in its own module and processes a single IMAP
//! command (CAPABILITY, LIST, LOGIN, LOGOUT, NOOP, SELECT, UID
//! SEARCH, UID FETCH, UID STORE, UID COPY, EXPUNGE).

mod capability;
mod expunge;
mod list;
mod login;
mod logout;
mod noop;
mod select;
mod uid_copy;
mod uid_fetch;
mod uid_search;
mod uid_store;

pub use capability::handle_capability;
pub use expunge::handle_expunge;
pub use list::handle_list;
pub use login::handle_login;
pub use logout::handle_logout;
pub use noop::handle_noop;
pub use select::handle_select;
pub use uid_copy::handle_uid_copy;
pub use uid_fetch::handle_uid_fetch;
pub use uid_search::handle_uid_search;
pub use uid_store::{StoreArgs, handle_uid_store};
