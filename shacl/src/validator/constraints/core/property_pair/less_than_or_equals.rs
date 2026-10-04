use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::validator::constraints::display;
use crate::validator::constraints::result_message;
use crate::validator::constraints::{ConstraintComponent, Parameters};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::SHACLPath;
#[cfg(feature = "sparql")]
use rudof_rdf::query::QueryRDF;
use rudof_rdf::term::{Object, Triple};
use std::fmt::Debug;

/// `sh:less_than_or_equals` — each value node is smaller than or equal to the objects of `<focus, iri, ?>`.
pub(crate) struct LessThanOrEquals<'a>(pub &'a IriS);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for LessThanOrEquals<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn parameters(&self, schema: &IRSchema) -> Parameters {
        [("lessThanOrEquals", display(schema, &Object::Iri(self.0.clone())))].into()
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
        schema: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        let component_iri = IriS::from(component);
        let parameters = <Self as ConstraintComponent<S>>::parameters(self, schema);
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
                            // Values that cannot be compared violate it too.
                            let violates = node2.sparql_compare(&node1).is_none_or(|ord| ord.is_gt());

                            if violates {
                                let node_obj = S::term_as_object(value).ok();
                                let vr = ValidationResult::new(
                                    fnode_obj.clone(),
                                    component.clone(),
                                    shape.severity().clone(),
                                )
                                .with_message(result_message(
                                    schema,
                                    shape,
                                    &component_iri,
                                    &parameters,
                                    node_obj.as_ref(),
                                ))
                                .with_path(maybe_path.cloned())
                                .with_source(Some(shape.id().clone()))
                                .with_value(node_obj);
                                validation_results.push(vr);
                            }
                        }
                    }
                },
                Err(_) => {
                    let vr = ValidationResult::new(fnode_obj, component.clone(), shape.severity().clone())
                        .with_path(maybe_path.cloned())
                        .with_message(result_message(schema, shape, &component_iri, &parameters, None))
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
