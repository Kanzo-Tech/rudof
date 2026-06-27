//! `projectForm`: from a focus node, evaluate every property path of a node
//! shape against the data graph, yielding each path's values (with a nested
//! sub-focus for `sh:node` references). A generic SHACL property-path projection.

use oxrdf::{NamedOrBlankNode, Term as OxTerm};
use rudof_rdf::rdf_core::SHACLPath;
use rudof_rdf::rdf_impl::OxigraphInMemory;
use shacl::ast::{ASTComponent, ASTSchema, ASTShape};

use crate::dto::{ProjectedForm, ProjectedProperty, ProjectedValue, TermValue};
use crate::shapes::path_key;
use crate::{object_to_value, term_to_object};

pub fn project_form(
    ast: &ASTSchema,
    graph: &OxigraphInMemory,
    focus: &TermValue,
    shape_id: &str,
) -> ProjectedForm {
    let focus_term = term_to_object(focus);
    let mut properties = Vec::new();

    if let Some(node) = find_node_shape(ast, shape_id) {
        for pref in node.property_shapes() {
            if let Some(ASTShape::PropertyShape(ps)) = ast.get_shape(pref) {
                let path = ps.path();
                let has_node = ps.components().iter().any(|c| matches!(c, ASTComponent::Node(_)));
                let values = eval_path(graph, &focus_term, path)
                    .into_iter()
                    .map(|n| {
                        let nested = if has_node && is_resource(&n) { Some(object_to_value(&n)) } else { None };
                        ProjectedValue { value: object_to_value(&n), nested }
                    })
                    .collect();
                properties.push(ProjectedProperty { path_key: path_key(path), values });
            }
        }
    }

    ProjectedForm { focus: focus.clone(), properties }
}

fn find_node_shape<'a>(ast: &'a ASTSchema, shape_id: &str) -> Option<&'a shacl::ast::ASTNodeShape> {
    ast.iter().find_map(|(id, shape)| match shape {
        ASTShape::NodeShape(ns) if object_iri(id).as_deref() == Some(shape_id) => Some(&**ns),
        _ => None,
    })
}

fn object_iri(o: &rudof_rdf::rdf_core::term::Object) -> Option<String> {
    match o {
        rudof_rdf::rdf_core::term::Object::Iri(i) => Some(i.as_str().to_string()),
        _ => None,
    }
}

fn is_resource(t: &OxTerm) -> bool {
    matches!(t, OxTerm::NamedNode(_) | OxTerm::BlankNode(_))
}

fn as_subject(t: &OxTerm) -> Option<NamedOrBlankNode> {
    match t {
        OxTerm::NamedNode(n) => Some(NamedOrBlankNode::NamedNode(n.clone())),
        OxTerm::BlankNode(b) => Some(NamedOrBlankNode::BlankNode(b.clone())),
        _ => None,
    }
}

fn subject_to_term(s: &NamedOrBlankNode) -> OxTerm {
    match s {
        NamedOrBlankNode::NamedNode(n) => OxTerm::NamedNode(n.clone()),
        NamedOrBlankNode::BlankNode(b) => OxTerm::BlankNode(b.clone()),
    }
}

/// Evaluate a SHACL property path from `node` against the graph → target nodes.
fn eval_path(graph: &OxigraphInMemory, node: &OxTerm, path: &SHACLPath) -> Vec<OxTerm> {
    match path {
        SHACLPath::Predicate { pred } => {
            let Some(subj) = as_subject(node) else { return vec![] };
            graph
                .quads()
                .filter(|q| q.subject == subj && q.predicate.as_str() == pred.as_str())
                .map(|q| q.object.clone())
                .collect()
        }
        SHACLPath::Inverse { path } => match &**path {
            SHACLPath::Predicate { pred } => graph
                .quads()
                .filter(|q| q.predicate.as_str() == pred.as_str() && &q.object == node)
                .map(|q| subject_to_term(&q.subject))
                .collect(),
            _ => Vec::new(),
        },
        SHACLPath::Sequence { paths } => {
            let mut current = vec![node.clone()];
            for step in paths {
                current = current.iter().flat_map(|n| eval_path(graph, n, step)).collect();
            }
            current
        }
        SHACLPath::Alternative { paths } => {
            paths.iter().flat_map(|p| eval_path(graph, node, p)).collect()
        }
        SHACLPath::ZeroOrMore { path } => closure(graph, node, path, true),
        SHACLPath::OneOrMore { path } => closure(graph, node, path, false),
        SHACLPath::ZeroOrOne { path } => {
            let mut v = vec![node.clone()];
            v.extend(eval_path(graph, node, path));
            v
        }
    }
}

fn closure(graph: &OxigraphInMemory, start: &OxTerm, step: &SHACLPath, include_start: bool) -> Vec<OxTerm> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut stack: Vec<OxTerm> = if include_start {
        vec![start.clone()]
    } else {
        eval_path(graph, start, step)
    };
    while let Some(n) = stack.pop() {
        let key = format!("{n}");
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(n.clone());
        for next in eval_path(graph, &n, step) {
            stack.push(next);
        }
    }
    out
}
