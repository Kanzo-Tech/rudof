mod oxigraph;

mod backend;
mod backend_error;

#[cfg(all(not(target_family = "wasm"), feature = "sparql"))]
pub use oxigraph::{OxigraphEndpoint, OxigraphEndpointError, SparqlVars};
pub use oxigraph::{OxigraphInMemory, OxigraphInMemoryError, ReaderMode};

pub use backend::RdfBackend;
pub use backend_error::RdfBackendError;

#[cfg(test)]
mod tests {
    mod in_memory_tests;
}
