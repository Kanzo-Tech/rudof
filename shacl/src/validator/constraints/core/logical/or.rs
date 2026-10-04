use crate::error::ValidationError;
use crate::ir::components::Or;
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

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Or {
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
        let component = Object::iri(component.into());

        for (fnode, nodes) in value_nodes.iter() {
            let fnode_obj = S::term_as_object(fnode)?;
            for node in nodes.iter() {
                let focus_nodes = FocusNodes::single(node.clone());
                let mut conforms = false;
                for idx in self.shapes().iter() {
                    let or_shape = shapes_graph.get_shape_from_idx_e(idx)?;
                    if or_shape
                        .validate(store, engine, Some(&focus_nodes), Some(shape), shapes_graph)?
                        .is_empty()
                    {
                        conforms = true;
                        break;
                    }
                }
                if !conforms {
                    let node_obj = S::term_as_object(node).ok();
                    let vr = ValidationResult::new(fnode_obj.clone(), component.clone(), shape.severity().clone())
                        .with_message(result_message(
                            shapes_graph,
                            shape,
                            &component_iri,
                            &[],
                            node_obj.as_ref(),
                        ))
                        .with_path(maybe_path.cloned())
                        .with_value(node_obj)
                        .with_source(Some(shape.id().clone()));
                    validation_results.push(vr);
                }
            }
        }

        Ok(validation_results)
    }
}
