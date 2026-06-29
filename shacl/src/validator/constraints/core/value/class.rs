use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::{Object, Term};
use rudof_rdf::vocab::{RdfVocab, RdfsVocab};
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

/// `sh:class` — each value node is a SHACL instance of the given class.
pub(crate) struct Class<'a>(pub &'a Object);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Class<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        if vn.is_literal() {
            return Ok(Check::Violate);
        }
        let term = S::object_as_term(self.0);
        let conforms = cx
            .store
            .objects_for(vn, &RdfVocab::rdf_type().into())
            .unwrap_or_default()
            .iter()
            .any(|ctype| {
                ctype == &term
                    || cx
                        .store
                        .objects_for(ctype, &RdfsVocab::rdfs_subclass_of_str().into())
                        .unwrap_or_default()
                        .contains(&term)
            });
        Ok(if conforms { Check::Hold } else { Check::Violate })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!("Class constraint not satisfied for class {}", self.0)
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
            formatdoc! {"
                PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
                PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
                ASK {{ {} rdf:type/rdfs:subClassOf* {} }}
            ", vn, self.0
            }
        };
        sparql_ask(
            component,
            shape,
            store,
            value_nodes,
            query_fn,
            &format!("Class constraint not satisfied for class {}", self.0),
            maybe_path,
        )
    }
}
