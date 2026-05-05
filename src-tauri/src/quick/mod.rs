#[cfg(feature = "quick")]
pub mod quick_connect;
#[cfg(feature = "quick")]
pub use quick_connect::{connect, QuickConnection, QuickError};
