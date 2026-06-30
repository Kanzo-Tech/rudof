//! `projectForm`: from a focus node, walk every property shape of a node shape
//! and evaluate its SHACL path against the data graph, yielding each path's
//! values (with a nested sub-focus for `sh:node` references). The path evaluation
//! itself runs in the façade (`FormEngine::eval_path`); this file keeps only the
//! form-shape walk and the value marshalling.

use rudof_lib::form::{ASTComponent, ASTNodeShape, ASTSchema, ASTShape, FormEngine, Object, Term as OxTerm};

use crate::dto::{ProjectedForm, ProjectedProperty, ProjectedValue, TermValue};
use crate::shapes::path_key;
use crate::{object_to_value, term_to_object};

pub fn project_form(engine: &FormEngine, ast: &ASTSchema, focus: &TermValue, shape_id: &str) -> ProjectedForm {
    let focus_term = term_to_object(focus);
    let mut properties = Vec::new();

    if let Some(node) = find_node_shape(ast, shape_id) {
        for pref in node.property_shapes() {
            if let Some(ASTShape::PropertyShape(ps)) = ast.get_shape(pref) {
                let path = ps.path();
                let has_node = ps.components().iter().any(|c| matches!(c, ASTComponent::Node(_)));
                let values = engine
                    .eval_path(&focus_term, path)
                    .into_iter()
                    .map(|n| {
                        let nested = if has_node && is_resource(&n) {
                            Some(object_to_value(&n))
                        } else {
                            None
                        };
                        ProjectedValue {
                            value: object_to_value(&n),
                            nested,
                        }
                    })
                    .collect();
                properties.push(ProjectedProperty {
                    path_key: path_key(path),
                    values,
                });
            }
        }
    }

    ProjectedForm {
        focus: focus.clone(),
        properties,
    }
}

fn find_node_shape<'a>(ast: &'a ASTSchema, shape_id: &str) -> Option<&'a ASTNodeShape> {
    ast.iter().find_map(|(id, shape)| match shape {
        ASTShape::NodeShape(ns) if object_iri(id).as_deref() == Some(shape_id) => Some(&**ns),
        _ => None,
    })
}

fn object_iri(o: &Object) -> Option<String> {
    match o {
        Object::Iri(i) => Some(i.as_str().to_string()),
        _ => None,
    }
}

fn is_resource(t: &OxTerm) -> bool {
    matches!(t, OxTerm::NamedNode(_) | OxTerm::BlankNode(_))
}
