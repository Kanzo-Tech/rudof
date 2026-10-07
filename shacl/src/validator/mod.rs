//! SHACL validation: a shapes graph denoted once ([`crate::algebra`]) and
//! interpreted over the data, in memory ([`eval`]) or as SQL ([`sql`]).

pub(crate) mod error;
pub mod eval;
pub mod report;
#[cfg(feature = "sql")]
pub mod sql;

pub use eval::{Fragment, fragment, validate, validate_scoped, validate_shape};
