use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::ShaclValidationMode;
use crate::validator::engine::{NativeEngine, SparqlEngine};
use crate::validator::index::ClassIndex;
use crate::validator::processor::{ShaclProcessor, run};
use crate::validator::report::ValidationResult;
use sparql_service::RdfData;

// TODO - move to validation::algorithms module
#[derive(Debug)]
pub struct DataValidation {
    data: RdfData,
}

impl DataValidation {
    pub fn new(data: RdfData) -> Self {
        Self { data }
    }
}

impl ShaclProcessor<RdfData> for DataValidation {
    fn store(&self) -> &RdfData {
        &self.data
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

    fn prepare_store(&mut self) -> Result<(), ValidationError> {
        self.data.check_store().map_err(ValidationError::from)
    }
}

impl From<RdfData> for DataValidation {
    fn from(value: RdfData) -> Self {
        Self::new(value)
    }
}
