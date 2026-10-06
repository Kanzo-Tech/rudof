#![doc = include_str!("../README.md")]
#![deny(rust_2018_idioms)]

pub mod algebra;
pub mod ast;
pub mod ir;
pub mod messages;
pub mod rdf;
pub mod types;
pub mod validator;
pub mod vocab;

pub mod error {
    pub use crate::ast::error::*;
    pub use crate::ir::error::*;
    pub use crate::rdf::error::*;
    pub use crate::validator::error::*;
}
