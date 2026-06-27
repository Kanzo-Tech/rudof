#![doc = include_str!("../README.md")]
#![deny(rust_2018_idioms)]

pub mod ast;
pub mod ir;
pub mod rdf;
pub mod types;
// metadata-form fork: un-gate the validator on wasm so SHACL validation can run
// in the browser. Still needs rayon made sequential/optional on wasm (see
// validator/processor: par_iter → iter under cfg(target_family = "wasm")).
pub mod validator;

pub mod error {
    pub use crate::ast::error::*;
    pub use crate::ir::error::*;
    pub use crate::rdf::error::*;
    pub use crate::validator::error::*;
}
