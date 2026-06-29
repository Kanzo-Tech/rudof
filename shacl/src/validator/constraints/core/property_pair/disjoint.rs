use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::types::MessageMap;
use crate::validator::constraints::ConstraintComponent;
use crate::validator::engine::Engine;
use crate::validator::iteration::{IterationStrategy, ValueNodeIteration};
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
use rudof_iri::IriS;
use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::SHACLPath;
use rudof_rdf::rdf_core::term::{Object, Triple};
use std::fmt::Debug;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::query::QueryRDF;

/// `sh:disjoint` — the value-node set is disjoint from `<focus, iri, ?>`.
pub(crate) struct Disjoint<'a>(pub &'a IriS);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Disjoint<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn validate_native<E: Engine<S>>(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &S,
        _: &mut E,
        value_nodes: &ValueNodes<S>,
        _: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        _: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        let violates = |f: &S::Term, vn: &S::Term| {
            let subject = S::term_as_subject(f).unwrap();
            let iri: S::IRI = self.0.clone().into();
            let triples_to_compare = match store.triples_with_subject_predicate(&subject, &iri) {
                Ok(iter) => iter,
                Err(_) => return true,
            };

            for triple in triples_to_compare {
                let value1 = S::term_as_object(vn).unwrap();
                let value2 = S::term_as_object(triple.obj()).unwrap();
                if value1 == value2 {
                    return true;
                }
            }
            false
        };

        let strategy = ValueNodeIteration;
        let msg = format!("Disjoint failed. Property {}", self.0);
        let mut results = Vec::new();
        for (focus_node, item) in strategy.iterate(value_nodes) {
            let Ok(focus) = S::term_as_object(focus_node) else {
                continue;
            };
            if violates(focus_node, item) {
                let component_obj = Object::iri(component.into());
                let value = S::term_as_object(item).ok();
                results.push(
                    ValidationResult::new(focus, component_obj, shape.severity().clone())
                        .with_source(Some(shape.id().clone()))
                        .with_message(MessageMap::from(msg.as_str()))
                        .with_path(maybe_path.cloned())
                        .with_value(value),
                );
            }
        }
        Ok(results)
    }

    #[cfg(feature = "sparql")]
    fn validate_sparql(
        &self,
        _: &IRComponent,
        _: &IRShape,
        _: &S,
        _: &ValueNodes<S>,
        _: Option<&IRShape>,
        _: Option<&SHACLPath>,
        _: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError>
    where
        S: QueryRDF,
    {
        unimplemented!()
    }
}
