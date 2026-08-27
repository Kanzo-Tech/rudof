use crate::error::ValidationError;
use crate::ir::components::If;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::types::MessageMap;
use crate::validator::constraints::ConstraintComponent;
use crate::validator::constraints::with_shape_message;
use crate::validator::engine::{Engine, Validate};
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::{FocusNodes, ValueNodes};
use crate::validator::report::ValidationResult;
use rudof_rdf::NeighsRDF;
use rudof_rdf::SHACLPath;
use rudof_rdf::term::Object;
use std::fmt::Debug;

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for If {
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
        let mut validation_results = Vec::new();
        let component = Object::iri(component.into());

        for (fnode, nodes) in value_nodes.iter() {
            let fnode_obj = S::term_as_object(fnode)?;
            for node in nodes.iter() {
                let focus_nodes = FocusNodes::single(node.clone());

                // A node conforms to the condition shape when validating it
                // yields no results (same convention as Xone counting).
                let cond_shape = shapes_graph.get_shape_from_idx_e(self.cond())?;
                let cond_results = cond_shape.validate(store, engine, Some(&focus_nodes), Some(shape), shapes_graph);
                let conforms_cond = matches!(cond_results, Ok(ref results) if results.is_empty());

                // Pick the branch dictated by the condition; a missing branch is
                // "no constraint" (conforms).
                let branch_idx = if conforms_cond { self.then() } else { self.els() };
                let Some(branch_idx) = branch_idx else {
                    continue;
                };

                let branch_shape = shapes_graph.get_shape_from_idx_e(branch_idx)?;
                let branch_results =
                    branch_shape.validate(store, engine, Some(&focus_nodes), Some(shape), shapes_graph);
                let branch_ok = matches!(branch_results, Ok(ref results) if results.is_empty());

                if !branch_ok {
                    let node_obj = S::term_as_object(node).ok();
                    let msg = format!(
                        "Shape {}: sh:if constraint not satisfied for node {node} (condition {}, branch {})",
                        shape.id(),
                        if conforms_cond {
                            "true -> sh:then"
                        } else {
                            "false -> sh:else"
                        },
                        branch_shape.id()
                    );
                    let vr = ValidationResult::new(fnode_obj.clone(), component.clone(), shape.severity().clone())
                        .with_message(with_shape_message(MessageMap::from(msg), shape))
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
