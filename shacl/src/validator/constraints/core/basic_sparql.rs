use crate::error::ValidationError;
use crate::ir::components::BasicSparql;
use crate::ir::IRSchema;
use crate::validator::constraints::ConstraintComponent;
use crate::validator::iteration::ValueNodeIteration;
use rudof_rdf::rdf_core::NeighsRDF;
use std::fmt::Debug;
#[cfg(feature = "sparql")]
use crate::ir::{IRComponent, IRShape};
#[cfg(feature = "sparql")]
use crate::types::MessageMap;
#[cfg(feature = "sparql")]
use crate::validator::constraints::sparql::{inject_values_into_where, path_to_sparql};
#[cfg(feature = "sparql")]
use crate::validator::nodes::ValueNodes;
#[cfg(feature = "sparql")]
use crate::validator::report::ValidationResult;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::SHACLPath;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::query::QueryRDF;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::term::Object;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::term::literal::ConcreteLiteral;

/// `sh:sparql` — a SPARQL-based constraint.
///
/// It cannot be checked without a SPARQL engine, so the native template
/// short-circuits to no violations; the real evaluation lives in the
/// sparql-gated [`validate_sparql`](ConstraintComponent::validate_sparql)
/// override.
impl<S: NeighsRDF + Debug> ConstraintComponent<S> for BasicSparql {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn short_circuit(&self) -> bool {
        true
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
        if self.deactivated() == Some(true) {
            return Ok(Vec::new());
        }

        let prefix_header = self
            .prefixes()
            .map(|p| {
                p.iter()
                    .map(|(prefix, iri)| format!("PREFIX {prefix}: <{iri}>\n"))
                    .collect::<String>()
            })
            .unwrap_or_default();

        let path_str = maybe_path.map(path_to_sparql).unwrap_or_default();
        let select_with_path = self.select().replace("$PATH", &path_str);

        let constraint_component = Object::Iri(component.into());
        let mut results = Vec::new();

        for (focus_node, _) in value_nodes.iter() {
            let values_clause = format!("VALUES ?this {{ {} }}", focus_node);
            let query_body = inject_values_into_where(&select_with_path, &values_clause);
            let full_query = format!("{}{}", prefix_header, query_body);

            let solutions = store
                .query_select(&full_query)
                .map_err(ValidationError::select_query_error::<S>)?;

            for sol in solutions.iter() {
                if let Some(failure_term) = sol.find_solution("failure")
                    && let Ok(Object::Literal(ConcreteLiteral::BooleanLiteral(true))) =
                        S::term_as_object(failure_term)
                {
                    return Err(ValidationError::QueryError(
                        "SPARQL constraint produced a failure".to_string(),
                    ));
                }

                let result_focus = if let Some(this_term) = sol.find_solution("this") {
                    S::term_as_object(this_term)?
                } else {
                    S::term_as_object(focus_node)?
                };

                let result_path = sol
                    .find_solution("path")
                    .and_then(|t| {
                        if let Ok(Object::Iri(iri)) = S::term_as_object(t) {
                            Some(SHACLPath::Predicate { pred: iri })
                        } else {
                            None
                        }
                    })
                    .or_else(|| maybe_path.cloned());

                let value = sol
                    .find_solution("value")
                    .and_then(|t| S::term_as_object(t).ok())
                    .or_else(|| S::term_as_object(focus_node).ok());

                let message = if let Some(msg_term) = sol.find_solution("message") {
                    MessageMap::from(format!("{msg_term}"))
                } else {
                    self.message().cloned().unwrap_or_default()
                };

                results.push(
                    ValidationResult::new(result_focus, constraint_component.clone(), shape.severity().clone())
                        .with_source(Some(shape.id().clone()))
                        .with_path(result_path)
                        .with_value(value)
                        .with_message(message),
                );
            }
        }

        Ok(results)
    }
}
