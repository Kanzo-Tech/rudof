//! `projectForm`: from a focus node, walk every property shape of a node shape
//! and evaluate its SHACL path against the data graph, yielding each path's
//! values (with a nested sub-focus for `sh:node` references). The path evaluation
//! itself runs in the façade (`FormEngine::eval_path`); this file keeps only the
//! form-shape walk and the value marshalling.
//!
//! Besides the node shape's direct property shapes, this also projects the
//! property paths of every `sh:if` conditional branch (`sh:then` / `sh:else`), so
//! a conditional field's values are already available the moment its condition
//! activates. Which conditions currently hold is reported separately in
//! `ProjectedForm.satisfied`.

use std::collections::HashSet;

use rudof_lib::form::{
    ASTComponent, ASTNodeShape, ASTPropertyShape, ASTSchema, ASTShape, FormEngine, Object, SHACLPath, Term as OxTerm,
};

use crate::dto::{ProjectedForm, ProjectedProperty, ProjectedValue, TermValue};
use crate::shapes::{object_str, path_key};
use crate::{object_to_value, term_to_object};

pub fn project_form(engine: &FormEngine, ast: &ASTSchema, focus: &TermValue, shape_id: &str) -> ProjectedForm {
    let focus_term = term_to_object(focus);
    let mut properties = Vec::new();
    let mut satisfied = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();

    if let Some(node) = find_node_shape(ast, shape_id) {
        // Base: the node shape's direct property shapes.
        for pref in node.property_shapes() {
            if let Some(ASTShape::PropertyShape(ps)) = ast.get_shape(pref) {
                project_property(engine, &focus_term, ps, &mut properties, &mut seen_keys);
            }
        }

        // Conditionals: project every branch's property paths too (so the value
        // is ready when the branch activates), and record satisfied conditions.
        // TODO(perf): compile IR once per project_form (conforms_focus recompiles
        // the IR on each call; a form may hold several conditions).
        for c in node.components() {
            let ASTComponent::If { cond, then_, else_ } = c else {
                continue;
            };
            for ps in branch_property_shapes(then_.as_ref(), ast)
                .into_iter()
                .chain(branch_property_shapes(else_.as_ref(), ast))
            {
                project_property(engine, &focus_term, ps, &mut properties, &mut seen_keys);
            }
            if let Ok(cond_obj) = Object::try_from(focus_term.clone()) {
                if engine.conforms_focus(cond, &cond_obj).unwrap_or(false) {
                    satisfied.push(object_str(cond));
                }
            }
        }
    }

    ProjectedForm {
        focus: focus.clone(),
        properties,
        satisfied,
    }
}

/// Project one property shape's path from `focus` into `properties`, deduped by
/// `path_key` so a path that is both a base and a branch property is emitted once.
fn project_property(
    engine: &FormEngine,
    focus: &OxTerm,
    ps: &ASTPropertyShape,
    properties: &mut Vec<ProjectedProperty>,
    seen_keys: &mut HashSet<String>,
) {
    let path: &SHACLPath = ps.path();
    let key = path_key(path);
    if !seen_keys.insert(key.clone()) {
        return;
    }
    let has_node = ps.components().iter().any(|c| matches!(c, ASTComponent::Node(_)));
    let values = engine
        .eval_path(focus, path)
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
    properties.push(ProjectedProperty { path_key: key, values });
}

/// Resolve a `sh:then` / `sh:else` object to the AST property shapes it contributes:
/// a node-shape target → its `property_shapes()`; a direct property shape → itself.
fn branch_property_shapes<'a>(obj: Option<&Object>, ast: &'a ASTSchema) -> Vec<&'a ASTPropertyShape> {
    let Some(obj) = obj else { return Vec::new() };
    match ast.get_shape(obj) {
        Some(ASTShape::NodeShape(ns)) => ns
            .property_shapes()
            .iter()
            .filter_map(|pref| match ast.get_shape(pref) {
                Some(ASTShape::PropertyShape(ps)) => Some(ps.as_ref()),
                _ => None,
            })
            .collect(),
        Some(ASTShape::PropertyShape(ps)) => vec![ps.as_ref()],
        _ => Vec::new(),
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
