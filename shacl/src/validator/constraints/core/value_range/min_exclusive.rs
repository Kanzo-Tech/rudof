use crate::error::ValidationError;
use crate::ir::IRSchema;
#[cfg(feature = "sparql")]
use crate::ir::{IRComponent, IRShape};
use crate::validator::constraints::Parameters;
#[cfg(feature = "sparql")]
use crate::validator::constraints::sparql_ask;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
#[cfg(feature = "sparql")]
use crate::validator::nodes::ValueNodes;
#[cfg(feature = "sparql")]
use crate::validator::report::ValidationResult;
#[cfg(feature = "sparql")]
use indoc::formatdoc;
use rudof_rdf::NeighsRDF;
#[cfg(feature = "sparql")]
use rudof_rdf::SHACLPath;
#[cfg(feature = "sparql")]
use rudof_rdf::query::QueryRDF;
use rudof_rdf::term::literal::ConcreteLiteral;
use std::fmt::Debug;

/// `sh:MinExclusive` value-range constraint.
pub(crate) struct MinExclusive<'a>(pub &'a ConcreteLiteral);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for MinExclusive<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let violates = match S::term_as_sliteral(vn) {
            Ok(lit) => lit.sparql_compare(self.0).map(|o| o.is_le()).unwrap_or(true),
            Err(_) => true,
        };
        Ok(if violates { Check::Violate } else { Check::Hold })
    }

    fn parameters(&self, _schema: &IRSchema) -> Parameters {
        [("minExclusive", self.0.lexical_form())].into()
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
        schema: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>
    where
        S: QueryRDF,
    {
        let query_fn = |vn: &S::Term| {
            formatdoc! {
                " ASK {{ FILTER ({} < {}) }} ",
                vn, self.0
            }
        };
        let parameters = <Self as ConstraintComponent<S>>::parameters(self, schema);
        sparql_ask(
            component,
            shape,
            store,
            value_nodes,
            query_fn,
            schema,
            &parameters,
            maybe_path,
        )
    }
}
