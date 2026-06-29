use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::types::MessageMap;
use crate::validator::constraints::ConstraintComponent;
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
use rudof_iri::IriS;
use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::SHACLPath;
use rudof_rdf::rdf_core::term::{Object, Triple};
use std::fmt::Debug;
#[cfg(feature = "sparql")]
use rudof_rdf::rdf_core::query::QueryRDF;

/// `sh:less_than` — each value node is smaller than the objects of `<focus, iri, ?>`.
pub(crate) struct LessThan<'a>(pub &'a IriS);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for LessThan<'_> {
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
        let mut validation_results = Vec::new();
        let component = Object::iri(component.into());

        for (fnode, nodes) in value_nodes.iter() {
            let subject = S::term_as_subject(fnode)?;
            let iri: S::IRI = self.0.clone().into();
            let fnode_obj = S::term_as_object(fnode)?;

            match store.triples_with_subject_predicate(&subject, &iri) {
                Ok(triples_iter) => {
                    for triple in triples_iter {
                        let node1 = S::term_as_object(triple.obj())?;
                        for value in nodes.iter() {
                            let node2 = S::term_as_object(value)?;
                            let msg = match node2.sparql_compare(&node1) {
                                None => Some(format!(
                                    "LessThan constraint violated: {node1} is not comparable to {node2}"
                                )),
                                Some(ord) if ord.is_ge() => Some(format!(
                                    "LessThan constraint violated: {node1} is not less than {node2}"
                                )),
                                _ => None,
                            };

                            if let Some(msg) = msg {
                                let node_obj = S::term_as_object(value).ok();
                                let vr = ValidationResult::new(
                                    fnode_obj.clone(),
                                    component.clone(),
                                    shape.severity().clone(),
                                )
                                .with_message(MessageMap::from(msg))
                                .with_path(maybe_path.cloned())
                                .with_source(Some(shape.id().clone()))
                                .with_value(node_obj);
                                validation_results.push(vr);
                            }
                        }
                    }
                },
                Err(e) => {
                    let msg = format!(
                        "LessThan: Error trying to find triples for subject {subject} and predicate {}: {e}",
                        self.0
                    );
                    let vr = ValidationResult::new(fnode_obj, component.clone(), shape.severity().clone())
                        .with_path(maybe_path.cloned())
                        .with_message(MessageMap::from(msg))
                        .with_source(Some(shape.id().clone()));
                    validation_results.push(vr);
                },
            }
        }

        Ok(validation_results)
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
