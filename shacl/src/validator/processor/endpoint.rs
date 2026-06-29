use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::ShaclValidationMode;
use crate::validator::engine::{NativeEngine, SparqlEngine};
use crate::validator::index::ClassIndex;
use crate::validator::processor::{ShaclProcessor, run};
use crate::validator::report::ValidationResult;
use crate::validator::store::{Endpoint, Store};
use prefixmap::PrefixMap;
use rudof_rdf::rdf_impl::OxigraphEndpoint;

// TODO - Move to validation::algorithms module
/// The endpoint Graph Validation Algorithm
pub struct EndpointValidation {
    store: Endpoint,
}

impl EndpointValidation {
    pub fn new(iri: &str, pm: &PrefixMap) -> Result<Self, ValidationError> {
        Ok(Self {
            store: Endpoint::new(iri, pm)?,
        })
    }
}

impl ShaclProcessor<OxigraphEndpoint> for EndpointValidation {
    fn store(&self) -> &OxigraphEndpoint {
        self.store.store()
    }

    fn run_validation(
        store: &OxigraphEndpoint,
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

impl From<OxigraphEndpoint> for EndpointValidation {
    fn from(value: OxigraphEndpoint) -> Self {
        Self { store: value.into() }
    }
}
