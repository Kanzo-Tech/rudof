//! `projectForm`: from a focus node, walk every property shape of a node shape
//! and evaluate its SHACL path against the data graph, yielding each path's
//! values (with a nested sub-focus for `sh:node` references). The path evaluation
//! itself runs in the façade (`FormEngine::eval_path`); this file keeps only the
//! form-shape walk and the value marshalling.
//!
//! Besides the node shape's direct property shapes, this also projects the
//! property paths of every conditional branch (see `shapes::conditionals_of`), so
//! a conditional field's values are already available the moment its condition
//! activates. Which conditions currently hold is reported separately in
//! `ProjectedForm.satisfied`.

use std::collections::HashSet;

use rudof_lib::form::{
    ASTComponent, ASTNodeShape, ASTPropertyShape, ASTSchema, ASTShape, FormEngine, Object, SHACLPath, Term as OxTerm,
};

use crate::dto::{ProjectedForm, ProjectedProperty, ProjectedValue, TermValue};
use crate::index::Labels;
use crate::shapes::{branch_property_shapes, conditionals_of, object_str, path_key};
use crate::{object_to_value, term_to_object};

pub fn project_form(engine: &FormEngine, ast: &ASTSchema, focus: &TermValue, shape_id: &str) -> ProjectedForm {
    let focus_term = term_to_object(focus);
    let mut properties = Vec::new();
    let mut satisfied = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();
    // One pass over the data graph, for every property's predicate labels.
    let labels = Labels::from_quads(engine.quads());

    if let Some(node) = find_node_shape(ast, shape_id) {
        // Base: the node shape's direct property shapes.
        for pref in node.property_shapes() {
            if let Some(ASTShape::PropertyShape(ps)) = ast.get_shape(pref) {
                project_property(engine, &focus_term, ps, &labels, &mut properties, &mut seen_keys);
            }
        }

        // Conditionals: project every branch's property paths too (so the value
        // is ready when the branch activates), and record satisfied conditions.
        // TODO(perf): compile IR once per project_form (conforms_focus recompiles
        // the IR on each call; a form may hold several conditions).
        for c in conditionals_of(node, ast) {
            for ps in branch_property_shapes(c.then, ast)
                .into_iter()
                .chain(branch_property_shapes(c.els, ast))
            {
                project_property(engine, &focus_term, ps, &labels, &mut properties, &mut seen_keys);
            }
            if let Ok(focus_obj) = Object::try_from(focus_term.clone()) {
                if engine.conforms_focus(c.cond, &focus_obj).unwrap_or(false) {
                    satisfied.push(object_str(c.cond));
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
    labels: &Labels,
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
    let path_labels = match path {
        SHACLPath::Predicate { pred } => labels.of(pred.as_str()),
        _ => Vec::new(),
    };
    properties.push(ProjectedProperty {
        path_key: key,
        values,
        path_labels,
    });
}

/// Find a node shape by the id the shapes projection gave it — an IRI, or `_:b…`
/// for an anonymous one, which is what a conditional's `thenId` may be.
fn find_node_shape<'a>(ast: &'a ASTSchema, shape_id: &str) -> Option<&'a ASTNodeShape> {
    ast.iter().find_map(|(id, shape)| match shape {
        ASTShape::NodeShape(ns) if object_str(id) == shape_id => Some(&**ns),
        _ => None,
    })
}

fn is_resource(t: &OxTerm) -> bool {
    matches!(t, OxTerm::NamedNode(_) | OxTerm::BlankNode(_))
}
