use crate::error::ValidationError;
use crate::ir::components::MaxInclusive;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::validator::constraints::{NativeValidator, validate_with};
#[cfg(feature = "sparql")]
use crate::validator::constraints::{BasicSparqlValidator, validate_ask_with};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
#[cfg(feature = "sparql")]
use indoc::formatdoc;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::query::QueryRDF;
use rudof_rdf::rdf_core::{NeighsRDF, SHACLPath};
use std::fmt::Debug;

impl<S: NeighsRDF + Debug + 'static> NativeValidator<S> for MaxInclusive {
    fn validate_native(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        _: &S,
        _: &mut dyn Engine<S>,
        value_nodes: &ValueNodes<S>,
        _: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        _: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        validate_with(
            component,
            shape,
            value_nodes,
            ValueNodeIteration,
            |n| match S::term_as_sliteral(n) {
                Ok(lit) => lit.sparql_compare(self.max_inclusive()).map(|o| o.is_gt()).unwrap_or(true),
                Err(_) => true,
            },
            &format!("MaxInclusive({}) not satisfied", self.max_inclusive()),
            maybe_path,
        )
    }
}

#[cfg(feature = "sparql")]
impl<S: QueryRDF + Debug + 'static> BasicSparqlValidator<S> for MaxInclusive {
    fn validate_sparql(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &S,
        value_nodes: &ValueNodes<S>,
        _: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        _: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        let query_fn = |vn: &S::Term| {
            formatdoc! {
                " ASK {{ FILTER ({} >= {}) }} ",
                vn, self.max_inclusive()
            }
        };

        validate_ask_with(
            component,
            shape,
            store,
            value_nodes,
            query_fn,
            &format!("MaxInclusive({}) not satisfied", self.max_inclusive()),
            maybe_path,
        )
    }
}
