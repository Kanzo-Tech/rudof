use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::literal::ConcreteLiteral;
use std::fmt::Debug;
#[cfg(feature = "sparql")]
use crate::ir::{IRComponent, IRShape};
#[cfg(feature = "sparql")]
use crate::validator::constraints::sparql_ask;
#[cfg(feature = "sparql")]
use crate::validator::nodes::ValueNodes;
#[cfg(feature = "sparql")]
use crate::validator::report::ValidationResult;
#[cfg(feature = "sparql")]
use indoc::formatdoc;
#[cfg(feature = "sparql")]
use rudof_rdf::SHACLPath;
#[cfg(feature = "sparql")]
use rudof_rdf::query::QueryRDF;

/// `sh:MinInclusive` value-range constraint.
pub(crate) struct MinInclusive<'a>(pub &'a ConcreteLiteral);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for MinInclusive<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let violates = match S::term_as_sliteral(vn) {
            Ok(lit) => lit.sparql_compare(self.0).map(|o| o.is_lt()).unwrap_or(true),
            Err(_) => true,
        };
        Ok(if violates { Check::Violate } else { Check::Hold })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!("MinInclusive({}) not satisfied", self.0)
    }

    #[cfg(feature = "sparql")]
    fn validate_sparql(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &S,
        value_nodes: &ValueNodes<S>,
        _: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        _: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>
    where
        S: QueryRDF,
    {
        let query_fn = |vn: &S::Term| {
            formatdoc! {
                " ASK {{ FILTER ({} <= {}) }} ",
                vn, self.0
            }
        };
        sparql_ask(
            component,
            shape,
            store,
            value_nodes,
            query_fn,
            &format!("MinInclusive({}) not satisfied", self.0),
            maybe_path,
        )
    }
}
