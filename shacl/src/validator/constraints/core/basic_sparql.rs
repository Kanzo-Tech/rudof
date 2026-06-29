use crate::error::ValidationError;
use crate::ir::components::BasicSparql;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::validator::constraints::NativeValidator;
use crate::validator::engine::Engine;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
use rudof_rdf::rdf_core::{NeighsRDF, SHACLPath};
use std::fmt::Debug;

/// `sh:sparql` cannot be checked without a SPARQL engine, so the native pass
/// skips it (no violations). The actual evaluation lives in the sparql-gated
/// [`BasicSparqlValidator`](crate::validator::constraints::BasicSparqlValidator)
/// impl. Kept always-compiled because the native validator dispatch covers every
/// `IRComponent` variant, `BasicSparql` included.
impl<RDF: NeighsRDF + Debug + 'static> NativeValidator<RDF> for BasicSparql {
    fn validate_native<E: Engine<RDF>>(
        &self,
        _: &IRComponent,
        _: &IRShape,
        _: &RDF,
        _: &mut E,
        _: &ValueNodes<RDF>,
        _: Option<&IRShape>,
        _: Option<&SHACLPath>,
        _: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        Ok(Vec::new())
    }
}
