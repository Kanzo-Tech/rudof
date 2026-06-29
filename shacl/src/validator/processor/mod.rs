#[cfg(feature = "sparql")]
mod endpoint;
mod graph;
#[cfg(feature = "sparql")]
mod rdf_data;

use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::ShaclValidationMode;
use crate::validator::engine::{Engine, Validate};
use crate::validator::report::{ValidationReport, ValidationResult};
#[cfg(feature = "sparql")]
pub use endpoint::EndpointValidation;
pub use graph::GraphValidation;
// wasm has no threads: validate shapes sequentially there, in parallel elsewhere.
#[cfg(not(target_family = "wasm"))]
use rayon::prelude::*;
#[cfg(feature = "sparql")]
pub use rdf_data::DataValidation;
use rudof_rdf::rdf_core::NeighsRDF;
use std::fmt::Debug;

/// The basic operations of the SHACL Processor.
///
/// The ShaclProcessor trait is the one in charge of applying the SHACL
/// Validation algorithm. For this, first, the validation report is initiliazed
/// to empty, and, for each shape in the schema, the target nodes are
/// selected, and then, each validator for each constraint is applied.
pub trait ShaclProcessor<S: NeighsRDF + Debug + Sync> {
    fn store(&self) -> &S;

    /// Runs the validation with the concrete engine(s) appropriate for this
    /// processor and `mode`. Each impl builds its read-only context (e.g. the
    /// class index) on the stack and lends it to the generic [`run`] driver —
    /// no `Box<dyn Engine>`.
    fn run_validation(
        store: &S,
        shapes_graph: &IRSchema,
        mode: &ShaclValidationMode,
    ) -> Result<Vec<ValidationResult>, ValidationError>;

    /// Called once before validation begins. Implementations that need lazy
    /// initialization (e.g. building an in-memory SPARQL store from a graph)
    /// should do so here.
    fn prepare_store(&mut self) -> Result<(), ValidationError> {
        Ok(())
    }

    /// Executes the Validation of the provided Graph, in any of the supported
    /// formats, against the shapes graph passed as an argument. As a result,
    /// the Validation Report generated from the validation process is returned.
    ///
    /// Shapes are validated in parallel using topological level ordering derived
    /// from the dependency graph. Shapes within the same level have no
    /// dependency relationships and are validated concurrently, while successive
    /// levels are processed sequentially to ensure that each shape's sub-shapes
    /// are already validated (and cached) before the shape itself runs.
    ///
    /// # Arguments
    ///
    /// * `shapes_graph` - A compiled SHACL shapes graph
    /// * `mode` - The validation mode to be applied during the validation process
    fn validate(
        &mut self,
        shapes_graph: &IRSchema,
        mode: &ShaclValidationMode,
    ) -> Result<ValidationReport, ValidationError> {
        self.prepare_store()?;
        let store = self.store();

        let all_results = Self::run_validation(store, shapes_graph, mode)?;

        let mut pm = shapes_graph.prefix_map().clone();
        if let Some(store_pm) = store.prefixmap() {
            pm.merge(store_pm);
        }

        Ok(ValidationReport::new().with_results(all_results).with_prefixmap(pm))
    }
}

/// The generic, engine-agnostic validation driver shared by all processors.
///
/// Shapes are grouped by topological level so that a shape only depends on
/// strictly-lower levels. Off wasm, each level is validated in parallel: rayon
/// **borrows** `&store`/`&schema`/`&master` (all `Sync`) and every task **forks
/// its own owned engine inside the closure**, so the engine never crosses a
/// thread boundary (which is why `Engine` need not be `Send`) and there is no
/// `Arc`. On wasm a single engine is threaded through every level in identical
/// order — deterministic reports and full cross-shape memoization.
pub(crate) fn run<S, E>(
    store: &S,
    shapes_graph: &IRSchema,
    master: &E,
) -> Result<Vec<ValidationResult>, ValidationError>
where
    S: NeighsRDF + Debug + Sync,
    E: Engine<S> + Sync,
{
    let levels = shapes_graph.shapes_with_targets_by_level();

    #[cfg(not(target_family = "wasm"))]
    {
        let mut all_results = Vec::new();
        for level in &levels {
            let level_results: Vec<ValidationResult> = level
                .par_iter()
                .map(|idx| -> Result<Vec<ValidationResult>, ValidationError> {
                    let mut engine = master.fork(); // owned, per-task cache
                    let shape = shapes_graph.get_shape_from_idx_e(idx)?;
                    shape.validate(store, &mut engine, None, Some(shape), shapes_graph)
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect();
            all_results.extend(level_results); // level barrier: deterministic order
        }
        Ok(all_results)
    }

    #[cfg(target_family = "wasm")]
    {
        // One engine threaded through everything → full memoization, zero sync.
        let mut engine = master.fork();
        let mut all_results = Vec::new();
        for level in &levels {
            for idx in level {
                let shape = shapes_graph.get_shape_from_idx_e(idx)?;
                all_results.extend(shape.validate(store, &mut engine, None, Some(shape), shapes_graph)?);
            }
        }
        Ok(all_results)
    }
}
