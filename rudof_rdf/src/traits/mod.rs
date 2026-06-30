//! RDF abstraction traits shared by every backend.

mod async_rdf;
mod build_rdf;
mod matcher;
mod neighs_rdf;
mod rdf;

pub use async_rdf::AsyncRDF;
pub use build_rdf::BuildRDF;
pub use matcher::{Any, Matcher};
pub use neighs_rdf::NeighsRDF;
pub use rdf::Rdf;
