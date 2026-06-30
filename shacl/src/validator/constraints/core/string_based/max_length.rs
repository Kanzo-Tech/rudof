use crate::error::ValidationError;
use crate::ir::IRSchema;
#[cfg(feature = "sparql")]
use crate::ir::{IRComponent, IRShape};
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
use rudof_rdf::term::literal::Literal;
use rudof_rdf::term::{Iri, Term};
use std::fmt::Debug;

/// `sh:MaxLength` — string-length constraint on IRIs and literals (not bnodes).
pub(crate) struct MaxLength(pub isize);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for MaxLength {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let bound = self.0 as usize;
        let violates = if vn.is_blank_node() {
            true
        } else if vn.is_iri() {
            match S::term_as_iri(vn) {
                Ok(iri) => iri.as_str().len() > bound,
                Err(_) => true,
            }
        } else if vn.is_literal() {
            match S::term_as_literal(vn) {
                Ok(lit) => lit.lexical_form().len() > bound,
                Err(_) => true,
            }
        } else {
            true
        };
        Ok(if violates { Check::Violate } else { Check::Hold })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!("MaxLength({}) not satisfied", self.0)
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
                " ASK {{ FILTER (STRLEN(str({})) <= {}) }} ",
                vn, self.0
            }
        };
        sparql_ask(
            component,
            shape,
            store,
            value_nodes,
            query_fn,
            &format!("MaxLength({}) not satisfied", self.0),
            maybe_path,
        )
    }
}
