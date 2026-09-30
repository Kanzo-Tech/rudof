use crate::error::ValidationError;
use crate::ir::components::Node;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::validator::constraints::ConstraintComponent;
use crate::validator::constraints::result_message;
use crate::validator::engine::{Engine, Validate};
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::{FocusNodes, ValueNodes};
use crate::validator::report::ValidationResult;
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use std::fmt::Debug;

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Node {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn validate_native<E: Engine<S>>(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        store: &S,
        engine: &mut E,
        value_nodes: &ValueNodes<S>,
        _: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        shapes_graph: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        let component_iri = IriS::from(component);
        let mut validation_results = Vec::new();
        let shape_idx = self.shape();
        let node_shape = shapes_graph.get_shape_from_idx_e(shape_idx)?;
        let component_obj = Object::iri(component.into());

        for (fnode, nodes) in value_nodes.iter() {
            let fnode_obj = S::term_as_object(fnode)?;
            for node in nodes.iter() {
                let node_object = S::term_as_object(node)?;
                let focus_nodes = FocusNodes::single(node.clone());

                let had_violations = if engine.has_validated(&node_object, *shape_idx) {
                    engine
                        .get_cached_results(&node_object, *shape_idx)
                        .map(|r| !r.is_empty())
                        .unwrap_or(false)
                } else {
                    let inner_results =
                        node_shape.validate(store, engine, Some(&focus_nodes), Some(shape), shapes_graph);
                    match inner_results {
                        Ok(results) => !results.is_empty(),
                        Err(e) => return Err(e),
                    }
                };

                if had_violations {
                    let vr = ValidationResult::new(fnode_obj.clone(), component_obj.clone(), shape.severity().clone())
                        .with_path(maybe_path.cloned())
                        .with_message(result_message(
                            shapes_graph,
                            shape,
                            &component_iri,
                            &[],
                            Some(&node_object),
                        ))
                        .with_value(Some(node_object.clone()))
                        .with_source(Some(shape.id().clone()));
                    validation_results.push(vr);
                }
            }
        }

        Ok(validation_results)
    }
}
