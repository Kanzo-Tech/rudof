mod oxigraph;

mod backend_error;
mod rdf_backend;

#[cfg(all(not(target_family = "wasm"), feature = "sparql"))]
pub use oxigraph::{OxigraphEndpoint, OxigraphEndpointError, SparqlVars};
pub use oxigraph::{OxigraphInMemory, OxigraphInMemoryError, ReaderMode};

pub use backend_error::RdfBackendError;
pub use rdf_backend::RdfBackend;

#[cfg(test)]
mod tests {
    mod in_memory_tests;
}
