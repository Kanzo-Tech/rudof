use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::types::NodeKind;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::Term;
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

/// `sh:nodeKind` — each value node has the given RDF node kind.
pub(crate) struct Nodekind<'a>(pub &'a NodeKind);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Nodekind<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let conforms = match (vn.is_blank_node(), vn.is_iri(), vn.is_literal()) {
            (true, false, false) => matches!(
                self.0,
                NodeKind::BNode | NodeKind::BNodeOrIri | NodeKind::BNodeOrLit
            ),
            (false, true, false) => matches!(self.0, NodeKind::Iri | NodeKind::BNodeOrIri | NodeKind::IriOrLit),
            (false, false, true) => matches!(self.0, NodeKind::Lit | NodeKind::IriOrLit | NodeKind::BNodeOrLit),
            _ => false,
        };
        Ok(if conforms { Check::Hold } else { Check::Violate })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!("NodeKind constraint not satisfied. Expected node kind: {}", self.0)
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
            if vn.is_iri() {
                formatdoc! {"
                    PREFIX sh: <http://www.w3.org/ns/shacl#>
                    ASK {{ FILTER ({} IN ( sh:IRI, sh:BlankNodeOrIRI, sh:IRIOrLiteral ) ) }}
                ", self.0
                }
            } else if vn.is_literal() {
                formatdoc! {"
                    PREFIX sh: <http://www.w3.org/ns/shacl#>
                    ASK {{ FILTER ({} IN ( sh:Literal, sh:BlankNodeOrLiteral, sh:IRIOrLiteral ) ) }}
                ", self.0
                }
            } else {
                formatdoc! {"
                    PREFIX sh: <http://www.w3.org/ns/shacl#>
                    ASK {{ FILTER ({} IN ( sh:BlankNode, sh:BlankNodeOrIRI, sh:BlankNodeOrLiteral ) ) }}
                ", self.0
                }
            }
        };
        sparql_ask(
            component,
            shape,
            store,
            value_nodes,
            query_fn,
            &format!("NodeKind constraint not satisfied. Expected node kind: {}", self.0),
            maybe_path,
        )
    }
}
