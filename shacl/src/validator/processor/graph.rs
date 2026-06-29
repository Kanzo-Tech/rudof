use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::ShaclValidationMode;
#[cfg(feature = "sparql")]
use crate::validator::engine::SparqlEngine;
use crate::validator::engine::NativeEngine;
use crate::validator::index::ClassIndex;
use crate::validator::processor::{ShaclProcessor, run};
use crate::validator::report::ValidationResult;
use crate::validator::store::{Graph, Store};
#[cfg(not(target_family = "wasm"))]
use rudof_rdf::rdf_core::RDFFormat;
#[cfg(not(feature = "sparql"))]
use rudof_rdf::rdf_impl::OxigraphInMemory;
#[cfg(feature = "sparql")]
use sparql_service::RdfData;
#[cfg(not(target_family = "wasm"))]
use std::path::Path;

// TODO - move to validation::algorithm module
/// The In-Memory Graph Validation algorithm
pub struct GraphValidation {
    store: Graph,
}

impl GraphValidation {
    pub fn new(store: Graph) -> Self {
        Self { store }
    }

    /// Returns an In-Memory Graph validation SHACL processor.
    ///
    /// # Arguments
    ///
    /// * `data` - A path to the graph's serialization file
    /// * `data_format` - Any of the possible RDF serialization formats
    /// * `base` - An optional String, the base URI
    /// * `mode` - Any of the possible SHACL validation modes
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// use shacl::validator::processor::GraphValidation;
    /// use shacl::validator::ShaclValidationMode;
    /// use shacl::validator::processor::ShaclProcessor;
    /// use rudof_rdf::rdf_core::RDFFormat;
    ///
    /// let graph_validation = GraphValidation::from_path(
    ///     "../examples/book_conformant.ttl", // example graph (refer to the examples folder)
    ///     RDFFormat::Turtle, // serialization format of the graph
    ///     None, // no base is defined
    /// );
    /// ```
    #[cfg(not(target_family = "wasm"))]
    pub fn from_path<P: AsRef<Path>>(path: P, format: RDFFormat, base: Option<&str>) -> Result<Self, ValidationError> {
        let store = Graph::from_path(path.as_ref(), &format, base)?;
        Ok(Self { store })
    }
}

#[cfg(feature = "sparql")]
impl ShaclProcessor<RdfData> for GraphValidation {
    fn store(&self) -> &RdfData {
        self.store.store()
    }

    fn run_validation(
        store: &RdfData,
        shapes_graph: &IRSchema,
        mode: &ShaclValidationMode,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        match mode {
            ShaclValidationMode::Native => {
                let index = ClassIndex::build(store)?;
                let master = NativeEngine::new(Some(&index));
                run(store, shapes_graph, &master)
            },
            ShaclValidationMode::Sparql => {
                let master = SparqlEngine::new();
                run(store, shapes_graph, &master)
            },
        }
    }
}

// Without the `sparql` feature the store is a plain in-memory graph and only the
// native engine is available. This is the path used by the wasm build.
#[cfg(not(feature = "sparql"))]
impl ShaclProcessor<OxigraphInMemory> for GraphValidation {
    fn store(&self) -> &OxigraphInMemory {
        self.store.store()
    }

    fn run_validation(
        store: &OxigraphInMemory,
        shapes_graph: &IRSchema,
        _mode: &ShaclValidationMode,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        // Without the `sparql` feature only the native engine is available.
        let index = ClassIndex::build(store)?;
        let master = NativeEngine::new(Some(&index));
        run(store, shapes_graph, &master)
    }
}

impl From<Graph> for GraphValidation {
    fn from(value: Graph) -> Self {
        Self::new(value)
    }
}
