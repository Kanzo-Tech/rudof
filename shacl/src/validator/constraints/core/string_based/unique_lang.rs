use crate::error::ValidationError;
use crate::ir::{IRComponent, IRSchema, IRShape};
use crate::types::MessageMap;
use crate::validator::constraints::ConstraintComponent;
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use crate::validator::nodes::ValueNodes;
use crate::validator::report::ValidationResult;
use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::SHACLPath;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_core::term::literal::Literal;
use std::collections::HashMap;
use std::fmt::Debug;

/// `sh:uniqueLang` — no two literal value nodes share a language tag.
///
/// Bespoke whole-set scan per focus node (overrides the per-item template).
pub(crate) struct UniqueLang(pub bool);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for UniqueLang {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn validate_native<E: Engine<S>>(
        &self,
        component: &IRComponent,
        shape: &IRShape,
        _: &S,
        _: &mut E,
        value_nodes: &ValueNodes<S>,
        _: Option<&IRShape>,
        maybe_path: Option<&SHACLPath>,
        _: &IRSchema,
    ) -> Result<Vec<ValidationResult>, ValidationError> {
        if !self.0 {
            return Ok(Default::default());
        }

        let mut validation_results = Vec::new();
        let component = Object::iri(component.into());

        for (fnode, nodes) in value_nodes.iter() {
            let fnode_obj = S::term_as_object(fnode)?;
            let mut langs_map: HashMap<String, Vec<S::Term>> = HashMap::new();
            for node in nodes.iter() {
                if let Ok(lit) = S::term_as_literal(node)
                    && let Some(lang) = lit.lang()
                {
                    langs_map.entry(lang.to_string()).or_default().push(node.clone());
                }
            }

            for (k, v) in langs_map {
                if v.len() > 1 {
                    let msg = format!(
                        "Unique lang failed for lang {k} with values: {}",
                        v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
                    );
                    let vr = ValidationResult::new(fnode_obj.clone(), component.clone(), shape.severity().clone())
                        .with_path(maybe_path.cloned())
                        .with_message(MessageMap::from(msg))
                        .with_source(Some(shape.id().clone()));
                    validation_results.push(vr);
                }
            }
        }

        Ok(validation_results)
    }
}
