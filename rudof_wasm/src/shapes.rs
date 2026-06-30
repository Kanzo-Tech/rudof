//! Maps rudof's SHACL AST (`shacl::ast`) into the vocabulary-agnostic
//! `ShapeModelJson` — node/property shapes with typed constraint cores, an open
//! component bag, and the standard SHACL/DASH presentation annotations
//! (`sh:name`/`order`/`group`, `shui:editor`/`viewer`) read from the shapes
//! graph. Emitting JSON here keeps the JS side from re-parsing SHACL.

use rudof_lib::form::{
    ASTComponent, ASTNodeShape, ASTPropertyShape, ASTSchema, ASTShape, BlankNode, ConcreteLiteral, IriRef, NamedNode,
    NamedOrBlankNode, NodeKind, Object, OxigraphInMemory, SHACLPath, Target, Term as OxTerm, Value,
};
use std::collections::HashMap;

use crate::dto::*;
use crate::object_to_value;

const SH: &str = "http://www.w3.org/ns/shacl#";
const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
const SHUI: &str = "http://www.w3.org/ns/shacl-ui#";
const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// rudof's SHACL parser is validation-focused and does not populate the
/// presentation/annotation terms (sh:name/description/order/group, shui:editor)
/// or sh:PropertyGroup metadata. We read those directly from the shapes graph.
pub fn schema_to_json(schema: &ASTSchema, graph: &OxigraphInMemory) -> ShapeModelJson {
    let mut node_shapes = Vec::new();
    let mut by_target_class = Vec::new();

    for (id, shape) in schema.iter() {
        if let ASTShape::NodeShape(ns) = shape {
            let ir = node_shape_to_ir(id, ns, schema, graph);
            for tc in &ir.target_classes {
                by_target_class.push((tc.clone(), ir.id.clone()));
            }
            node_shapes.push(ir);
        }
    }

    ShapeModelJson { node_shapes, groups: read_groups(graph), by_target_class }
}

fn node_shape_to_ir(id: &Object, ns: &ASTNodeShape, schema: &ASTSchema, graph: &OxigraphInMemory) -> NodeShapeIR {
    let target_classes: Vec<String> = ns
        .targets()
        .iter()
        .filter_map(|t| match t {
            Target::Class(o) | Target::ImplicitClass(o) => object_iri(o),
            _ => None,
        })
        .collect();

    let properties = ns
        .property_shapes()
        .iter()
        .filter_map(|pref| match schema.get_shape(pref) {
            Some(ASTShape::PropertyShape(ps)) => Some(property_to_ir(ps, schema, graph)),
            _ => None,
        })
        .collect();

    NodeShapeIR {
        id: object_str(id),
        instance_class: target_classes.first().cloned(),
        target_classes,
        properties,
        closed: None,
    }
}

fn property_to_ir(ps: &ASTPropertyShape, schema: &ASTSchema, graph: &OxigraphInMemory) -> PropertyShapeIR {
    let mut value = ValueConstraints::default();
    let mut logical = LogicalConstraints::default();
    let mut cardinality = Cardinality::default();
    let mut node = None;

    for c in ps.components() {
        match c {
            ASTComponent::Datatype(iri) => value.datatype = Some(iriref_str(iri)),
            ASTComponent::Class(o) => value.class_iri = object_iri(o),
            ASTComponent::NodeKind(nk) => value.node_kind = Some(nodekind_iri(nk)),
            ASTComponent::MinCount(n) => cardinality.min = Some(*n as i64),
            ASTComponent::MaxCount(n) => cardinality.max = Some(*n as i64),
            ASTComponent::MinLength(n) => value.min_length = Some(*n as i64),
            ASTComponent::MaxLength(n) => value.max_length = Some(*n as i64),
            ASTComponent::MinInclusive(l) => value.min_inclusive = concrete_f64(l),
            ASTComponent::MaxInclusive(l) => value.max_inclusive = concrete_f64(l),
            ASTComponent::MinExclusive(l) => value.min_exclusive = concrete_f64(l),
            ASTComponent::MaxExclusive(l) => value.max_exclusive = concrete_f64(l),
            ASTComponent::Pattern { pattern, flags } => {
                value.pattern = Some(pattern.clone());
                value.flags = flags.clone();
            }
            ASTComponent::UniqueLang(b) => value.unique_lang = Some(*b),
            ASTComponent::LanguageIn(langs) => {
                value.language_in = Some(langs.iter().map(|l| l.as_str().to_string()).collect())
            }
            ASTComponent::In(vals) => value.in_values = Some(vals.iter().map(value_to_term).collect()),
            ASTComponent::HasValue(v) => value.has_value = Some(value_to_term(v)),
            ASTComponent::Node(o) => node = object_iri(o),
            ASTComponent::Or(refs) => logical.or = Some(resolve_branches(refs, schema, graph)),
            ASTComponent::Xone(refs) => logical.xone = Some(resolve_branches(refs, schema, graph)),
            ASTComponent::And(refs) => logical.and = Some(resolve_branches(refs, schema, graph)),
            ASTComponent::Not(o) => {
                if let Some(ASTShape::PropertyShape(b)) = schema.get_shape(o) {
                    logical.not = Some(Box::new(property_to_ir(b, schema, graph)));
                }
            }
            _ => {}
        }
    }

    // sh:defaultValue is an annotation, not a validation constraint, so rudof's
    // parser doesn't surface it — read it from the shapes graph like the others.
    value.default_value = default_value(graph, ps.id());

    PropertyShapeIR {
        id: object_iri(ps.id()),
        path: path_to_ir(ps.path()),
        path_key: path_key(ps.path()),
        cardinality,
        value,
        logical,
        node,
        presentation: presentation(graph, ps.id()),
        components: read_components(graph, ps.id()),
    }
}

/// Read sh:defaultValue for a property-shape node from the shapes graph.
fn default_value(graph: &OxigraphInMemory, node: &Object) -> Option<TermValue> {
    let subj = object_to_subject(node)?;
    let pred = format!("{SH}defaultValue");
    graph
        .quads()
        .find(|q| q.subject == subj && q.predicate.as_str() == pred)
        .map(|q| object_to_value(&q.object))
}

/// Every (predicate, object) on the property-shape node, grouped by predicate IRI
/// — the open extension point so rules/widgets can read terms the typed core does
/// not model (custom vocab, new SHACL 1.2 components).
fn read_components(graph: &OxigraphInMemory, node: &Object) -> Vec<ComponentIR> {
    let Some(subj) = object_to_subject(node) else { return Vec::new() };
    let mut by_pred: HashMap<String, Vec<TermValue>> = HashMap::new();
    for q in graph.quads() {
        if q.subject != subj {
            continue;
        }
        by_pred.entry(q.predicate.as_str().to_string()).or_default().push(object_to_value(&q.object));
    }
    by_pred
        .into_iter()
        .map(|(iri, values)| {
            let mut params = HashMap::new();
            params.insert("value".to_string(), values);
            ComponentIR { iri, params }
        })
        .collect()
}

fn resolve_branches(refs: &[Object], schema: &ASTSchema, graph: &OxigraphInMemory) -> Vec<PropertyShapeIR> {
    refs
        .iter()
        .filter_map(|o| match schema.get_shape(o) {
            Some(ASTShape::PropertyShape(ps)) => Some(property_to_ir(ps, schema, graph)),
            _ => None,
        })
        .collect()
}

// ---- annotations read from the shapes graph ---------------------------------

fn object_to_subject(o: &Object) -> Option<NamedOrBlankNode> {
    match o {
        Object::Iri(i) => Some(NamedOrBlankNode::NamedNode(NamedNode::new_unchecked(i.as_str()))),
        Object::BlankNode(b) => Some(NamedOrBlankNode::BlankNode(BlankNode::new_unchecked(b))),
        _ => None,
    }
}

/// Read sh:name / sh:description / sh:order / sh:group / shui:editor|viewer for a
/// property-shape node from the shapes graph (rudof's AST omits these).
fn presentation(graph: &OxigraphInMemory, node: &Object) -> PresentationHints {
    let mut p = PresentationHints::default();
    let Some(subj) = object_to_subject(node) else { return p };

    for q in graph.quads() {
        if q.subject != subj {
            continue;
        }
        let pred = q.predicate.as_str();
        if pred == format!("{SH}name") {
            if let Some(ls) = lang_string(&q.object) {
                p.names.push(ls);
            }
        } else if pred == format!("{SH}description") {
            if let Some(ls) = lang_string(&q.object) {
                p.descriptions.push(ls);
            }
        } else if pred == format!("{SH}order") {
            p.order = literal_value(&q.object).and_then(|v| v.parse().ok());
        } else if pred == format!("{SH}group") {
            p.group_id = iri_value(&q.object);
        } else if pred == format!("{SHUI}editor") {
            p.editor = iri_value(&q.object);
        } else if pred == format!("{SHUI}viewer") {
            p.viewer = iri_value(&q.object);
        }
    }
    p
}

fn read_groups(graph: &OxigraphInMemory) -> Vec<PropertyGroupIR> {
    let group_type = format!("{SH}PropertyGroup");
    let mut groups = Vec::new();
    // Collect group subjects (rdf:type sh:PropertyGroup), then their label/order.
    let subjects: Vec<NamedOrBlankNode> = graph
        .quads()
        .filter(|q| q.predicate.as_str() == RDF_TYPE && term_iri(&q.object).as_deref() == Some(group_type.as_str()))
        .map(|q| q.subject.clone())
        .collect();
    for subj in subjects {
        let id = match &subj {
            NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
            NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
        };
        let mut labels = Vec::new();
        let mut order = None;
        for q in graph.quads() {
            if q.subject != subj {
                continue;
            }
            let pred = q.predicate.as_str();
            if pred == format!("{RDFS}label") {
                if let Some(ls) = lang_string(&q.object) {
                    labels.push(ls);
                }
            } else if pred == format!("{SH}order") {
                order = literal_value(&q.object).and_then(|v| v.parse().ok());
            }
        }
        groups.push(PropertyGroupIR { id, labels, order });
    }
    groups
}

fn lang_string(t: &OxTerm) -> Option<LangString> {
    match t {
        OxTerm::Literal(l) => Some(LangString {
            value: l.value().to_string(),
            language: l.language().unwrap_or("").to_string(),
        }),
        _ => None,
    }
}

fn literal_value(t: &OxTerm) -> Option<String> {
    match t {
        OxTerm::Literal(l) => Some(l.value().to_string()),
        _ => None,
    }
}

fn iri_value(t: &OxTerm) -> Option<String> {
    match t {
        OxTerm::NamedNode(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

fn term_iri(t: &OxTerm) -> Option<String> {
    iri_value(t)
}

// ---- path --------------------------------------------------------------------

fn path_to_ir(path: &SHACLPath) -> PathExpr {
    match path {
        SHACLPath::Predicate { pred } => PathExpr::Predicate { iri: pred.as_str().to_string() },
        SHACLPath::Inverse { path } => PathExpr::Inverse { of: Box::new(path_to_ir(path)) },
        SHACLPath::Sequence { paths } => PathExpr::Sequence { steps: paths.iter().map(path_to_ir).collect() },
        SHACLPath::Alternative { paths } => PathExpr::Alternative { options: paths.iter().map(path_to_ir).collect() },
        SHACLPath::ZeroOrMore { path } => PathExpr::ZeroOrMore { path: Box::new(path_to_ir(path)) },
        SHACLPath::OneOrMore { path } => PathExpr::OneOrMore { path: Box::new(path_to_ir(path)) },
        SHACLPath::ZeroOrOne { path } => PathExpr::ZeroOrOne { path: Box::new(path_to_ir(path)) },
    }
}

// ---- term/value conversions --------------------------------------------------

fn object_iri(o: &Object) -> Option<String> {
    match o {
        Object::Iri(i) => Some(i.as_str().to_string()),
        _ => None,
    }
}

/// Canonical path key matching the TS `pathKey` (used to align projected values
/// to their property shape). Mirrors SPARQL property-path surface syntax.
pub(crate) fn path_key(path: &SHACLPath) -> String {
    match path {
        SHACLPath::Predicate { pred } => pred.as_str().to_string(),
        SHACLPath::Inverse { path } => format!("^{}", path_key(path)),
        SHACLPath::Sequence { paths } => {
            format!("({})", paths.iter().map(path_key).collect::<Vec<_>>().join("/"))
        }
        SHACLPath::Alternative { paths } => {
            format!("({})", paths.iter().map(path_key).collect::<Vec<_>>().join("|"))
        }
        SHACLPath::ZeroOrMore { path } => format!("{}*", path_key(path)),
        SHACLPath::OneOrMore { path } => format!("{}+", path_key(path)),
        SHACLPath::ZeroOrOne { path } => format!("{}?", path_key(path)),
    }
}

fn object_str(o: &Object) -> String {
    match o {
        Object::Iri(i) => i.as_str().to_string(),
        Object::BlankNode(b) => format!("_:{b}"),
        Object::Literal(l) => concrete_lexical(l),
        _ => String::new(),
    }
}

fn iriref_str(iri: &IriRef) -> String {
    match iri {
        IriRef::Iri(i) => i.as_str().to_string(),
        IriRef::Prefixed { prefix, local } => format!("{prefix}:{local}"),
    }
}

fn nodekind_iri(nk: &NodeKind) -> String {
    let local = match nk {
        NodeKind::Iri => "IRI",
        NodeKind::Lit => "Literal",
        NodeKind::BNode => "BlankNode",
        NodeKind::BNodeOrIri => "BlankNodeOrIRI",
        NodeKind::BNodeOrLit => "BlankNodeOrLiteral",
        NodeKind::IriOrLit => "IRIOrLiteral",
    };
    format!("{SH}{local}")
}

fn value_to_term(v: &Value) -> TermValue {
    match v {
        Value::Iri(iri) => TermValue::named(&iriref_str(iri)),
        Value::Literal(l) => concrete_to_term(l),
    }
}

fn concrete_to_term(l: &ConcreteLiteral) -> TermValue {
    match l {
        ConcreteLiteral::StringLiteral { lexical_form, lang } => {
            TermValue::literal(lexical_form, None, lang.as_ref().map(|x| x.as_str().to_string()))
        }
        ConcreteLiteral::DatatypeLiteral { lexical_form, datatype }
        | ConcreteLiteral::WrongDatatypeLiteral { lexical_form, datatype, .. } => {
            TermValue::literal(lexical_form, Some(iriref_str(datatype)), None)
        }
        ConcreteLiteral::NumericLiteral(n) => TermValue::literal(&n.lexical_form(), None, None),
        ConcreteLiteral::DatetimeLiteral(d) => {
            TermValue::literal(&d.to_string(), Some(format!("{XSD}dateTime")), None)
        }
        ConcreteLiteral::BooleanLiteral(b) => {
            TermValue::literal(&b.to_string(), Some(format!("{XSD}boolean")), None)
        }
    }
}

fn concrete_lexical(l: &ConcreteLiteral) -> String {
    match l {
        ConcreteLiteral::StringLiteral { lexical_form, .. }
        | ConcreteLiteral::DatatypeLiteral { lexical_form, .. }
        | ConcreteLiteral::WrongDatatypeLiteral { lexical_form, .. } => lexical_form.clone(),
        ConcreteLiteral::NumericLiteral(n) => n.lexical_form(),
        ConcreteLiteral::DatetimeLiteral(d) => d.to_string(),
        ConcreteLiteral::BooleanLiteral(b) => b.to_string(),
    }
}

fn concrete_f64(l: &ConcreteLiteral) -> Option<f64> {
    concrete_lexical(l).parse().ok()
}
