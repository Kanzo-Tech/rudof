//! SHACL validation: a shapes graph denoted once ([`crate::algebra`]) and
//! interpreted over the data, in memory ([`eval`]) or as SQL ([`sql`]).

pub mod eval;
pub(crate) mod error;
pub mod report;
#[cfg(feature = "sql")]
pub mod sql;

pub use eval::{validate, validate_shape};
