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
use rudof_rdf::term::{Object, Term};
use std::fmt::Debug;

/// `sh:class` — each value node is a SHACL instance of the given class.
pub(crate) struct Class<'a>(pub &'a Object);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Class<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    /// <https://www.w3.org/TR/shacl/#ClassConstraintComponent>
    ///
    /// "Each value node is a SHACL instance of $class" (§4.4.1) — and §1.1 puts
    /// the *SHACL superclasses* of a node's types among its SHACL types, so the
    /// relation is the transitive `rdfs:subClassOf` closure, not one hop. A node
    /// typed `:Officer`, where `:Officer rdfs:subClassOf :Role` and
    /// `:Role rdfs:subClassOf skos:Concept`, conforms to `sh:class skos:Concept`;
    /// this used to walk a single `rdfs:subClassOf` hop and report a violation.
    ///
    /// The set is the engine's own: [`Engine::is_shacl_instance`] is the same
    /// closure `sh:targetClass` selects with, so a shape cannot target a node
    /// through the hierarchy and then deny it the class it was targeted by.
    fn check<E: Engine<S>>(&self, vn: &S::Term, cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        // A literal has no `rdf:type` triples, so it is nobody's SHACL instance.
        if vn.is_literal() {
            return Ok(Check::Violate);
        }
        let conforms = cx.engine.is_shacl_instance(cx.store, vn, self.0)?;
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
