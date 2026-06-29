use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::validator::constraints::ConstraintComponent;
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::{Object, Triple};
use std::collections::HashSet;
use std::fmt::Debug;
#[cfg(feature = "sparql")]
use rudof_rdf::query::QueryRDF;

/// `sh:equals` — the value-node set equals the objects of `<focus, iri, ?>`.
pub(crate) struct Equals<'a>(pub &'a IriS);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Equals<'_> {
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
        let component_obj = Object::iri(component.into());
        let mut results = Vec::new();

        for (fnode, nodes) in value_nodes.iter() {
            let subject = match S::term_as_subject(fnode) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let iri: S::IRI = self.0.clone().into();

            let prop_values = store
                .triples_with_subject_predicate(&subject, &iri)
                .map_err(ValidationError::new_graph_error::<S>)?
                .map(|t| t.obj().clone())
                .collect::<HashSet<_>>();

            let nodes_set = nodes.iter().collect::<HashSet<_>>();

            let fnode_obj = S::term_as_object(fnode)?;

            for pv in &prop_values {
                if !nodes_set.contains(pv) {
                    let value = S::term_as_object(pv).ok();
                    let vr = ValidationResult::new(fnode_obj.clone(), component_obj.clone(), shape.severity().clone())
                        .with_source(Some(shape.id().clone()))
                        .with_path(maybe_path.cloned())
                        .with_value(value);
                    results.push(vr);
                }
            }

            for vn in nodes.iter() {
                if !prop_values.contains(vn) {
                    let value = S::term_as_object(vn).ok();
                    let vr = ValidationResult::new(fnode_obj.clone(), component_obj.clone(), shape.severity().clone())
                        .with_source(Some(shape.id().clone()))
                        .with_path(maybe_path.cloned())
                        .with_value(value);
                    results.push(vr);
                }
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
