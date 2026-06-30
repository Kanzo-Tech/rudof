//! `rudof_rdf` — RDF model, traits, parsing and backends used across rudof.
//!
//! The crate is organised into a small set of top-level modules:
//! - [`model`] — RDF terms (`Object`, `Subject`, `Triple`, literals, …) and [`RDFFormat`].
//! - [`traits`] — the RDF abstraction traits ([`Rdf`], [`NeighsRDF`], [`BuildRDF`], …).
//! - [`path`] — SHACL property paths ([`SHACLPath`]).
//! - [`query`] — SPARQL query model and result types.
//! - [`parser`] — the RDF node parser combinators.
//! - [`vocab`] — well-known RDF/RDFS/OWL/SHACL/XSD vocabularies.
//! - [`backend`] — concrete RDF stores (oxigraph in-memory and SPARQL endpoints).
//!
//! The most frequently used types are re-exported at the crate root.

mod errors;
mod model;
mod path;
mod rdf_data_config;
mod traits;

pub mod backend;
pub mod parser;
pub mod query;
pub mod utils;
pub mod vocab;

pub use errors::RDFError;
pub use model::RDFFormat;
pub use model::term;
pub use parser::{ParseCtx, RdfFocus};
pub use path::SHACLPath;
pub use rdf_data_config::{EndpointDescription, RdfDataConfig};
pub use traits::{Any, AsyncRDF, BuildRDF, Matcher, NeighsRDF, Rdf};
