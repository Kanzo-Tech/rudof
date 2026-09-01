//! Serde DTOs that ARE the JSON contract crossing the wasm boundary (camelCase,
//! tagged path union). `serde-wasm-bindgen` marshals them to/from plain JS
//! objects. A vocabulary-agnostic view of SHACL shapes, projected nodes and
//! validation reports — consumers map them to their own model.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TermValue {
    #[serde(rename = "termType")]
    pub term_type: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

impl TermValue {
    pub fn named(v: &str) -> Self {
        Self {
            term_type: "NamedNode".into(),
            value: v.into(),
            datatype: None,
            language: None,
        }
    }
    pub fn blank(v: &str) -> Self {
        Self {
            term_type: "BlankNode".into(),
            value: v.into(),
            datatype: None,
            language: None,
        }
    }
    pub fn literal(v: &str, datatype: Option<String>, language: Option<String>) -> Self {
        Self {
            term_type: "Literal".into(),
            value: v.into(),
            datatype,
            language,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LangString {
    pub value: String,
    pub language: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PathExpr {
    Predicate { iri: String },
    Inverse { of: Box<PathExpr> },
    Sequence { steps: Vec<PathExpr> },
    Alternative { options: Vec<PathExpr> },
    ZeroOrMore { path: Box<PathExpr> },
    OneOrMore { path: Box<PathExpr> },
    ZeroOrOne { path: Box<PathExpr> },
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Cardinality {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ValueConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_iri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_inclusive: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_inclusive: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_exclusive: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_exclusive: Option<f64>,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    pub in_values: Option<Vec<TermValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_value: Option<TermValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<TermValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_lang: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_in: Option<Vec<String>>,
}

/// A node-shape-level `sh:if` / `sh:then` / `sh:else` conditional. The `then`/`else`
/// property shapes are the fields shown when the condition (does not) hold.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalIR {
    /// stable id of the sh:if condition shape (IRI, or "_:b" for a blank node).
    pub condition_id: String,
    #[serde(default)]
    pub then: Vec<PropertyShapeIR>,
    #[serde(default, rename = "else")]
    pub els: Vec<PropertyShapeIR>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct LogicalConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub or: Option<Vec<PropertyShapeIR>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xone: Option<Vec<PropertyShapeIR>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub and: Option<Vec<PropertyShapeIR>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not: Option<Box<PropertyShapeIR>>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PresentationHints {
    pub names: Vec<LangString>,
    pub descriptions: Vec<LangString>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_line: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ComponentIR {
    pub iri: String,
    pub params: HashMap<String, Vec<TermValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PropertyShapeIR {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub path: PathExpr,
    /// Canonical SPARQL-ish path key (`(a/b)`, `^p`), matching the projected
    /// `ProjectedProperty.pathKey` — lets consumers align a property shape to its
    /// projected values without re-deriving the key.
    pub path_key: String,
    pub cardinality: Cardinality,
    pub value: ValueConstraints,
    pub logical: LogicalConstraints,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub presentation: PresentationHints,
    pub components: Vec<ComponentIR>,
    /// `sh:deactivated true` (SHACL 2.1.6): the shape is switched off, so every
    /// term conforms to it and the validator reports nothing for it. Emitted so a
    /// form consumer can render nothing for it rather than collecting input that
    /// is never validated. `None` when the shape is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deactivated: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NodeShapeIR {
    pub id: String,
    pub target_classes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_class: Option<String>,
    pub properties: Vec<PropertyShapeIR>,
    #[serde(default)]
    pub conditionals: Vec<ConditionalIR>,
    /// `sh:closed true` (SHACL 4.8.1): the focus node may carry no property
    /// beyond those the shape declares. `None` when open (`sh:closed` absent, or
    /// stated `false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// `sh:ignoredProperties`: the predicates a closed shape permits anyway
    /// (`rdf:type` being the usual one). Meaningless without `closed`, and
    /// inseparable from it — SHACL parses the two as a single component, and a
    /// consumer enforcing `closed` without these would reject exactly what the
    /// profile went out of its way to allow. Sorted, so the payload is stable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored_properties: Vec<String>,
    /// `sh:deactivated true` (SHACL 2.1.6): the shape is switched off, so every
    /// term conforms to it and the validator reports nothing for it. Emitted so a
    /// form consumer can render nothing for it rather than collecting input that
    /// is never validated. `None` when the shape is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deactivated: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PropertyGroupIR {
    pub id: String,
    pub labels: Vec<LangString>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShapeModelJson {
    pub node_shapes: Vec<NodeShapeIR>,
    pub groups: Vec<PropertyGroupIR>,
    pub by_target_class: Vec<(String, String)>,
}

// ---- projection + validation (abi.ts) ---------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectedValue {
    pub value: TermValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<TermValue>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectedProperty {
    pub path_key: String,
    pub values: Vec<ProjectedValue>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectedForm {
    pub focus: TermValue,
    pub properties: Vec<ProjectedProperty>,
    /// condition_ids (matching `ConditionalIR.condition_id`) whose `sh:if`
    /// condition the focus currently conforms to — the live "satisfied" flag.
    #[serde(default)]
    pub satisfied: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RudofQuad {
    pub subject: TermValue,
    pub predicate: TermValue,
    pub object: TermValue,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RudofResult {
    pub focus_node: TermValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<TermValue>,
    /// The path as its canonical key — the SAME string `shapes::path_key` gives
    /// the projection, because the consumer files an error under
    /// `${focusNode}|${pathKey}` and a field it cannot match is an error that
    /// silently becomes node-level.
    ///
    /// `path` above carries only a predicate, so every inverse, sequence,
    /// alternative and quantified path reported its violation with no path at
    /// all. That was survivable while those fields were read-only. It is not
    /// now: hundreds of alternative-path fields carry `sh:minCount 1`, and a
    /// required field whose error cannot point at it is a form that says
    /// "something in here is wrong" and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<TermValue>,
    /// Lang-tagged messages: the engine's default (untagged, `language: ""`)
    /// merged with the shape's per-language `sh:message` entries. The JS side
    /// picks the best by locale; untagged is the fallback.
    pub message: Vec<LangString>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_constraint_component: Option<String>,
}

// The validation report crossing the ABI (also produced by the Node fake).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RudofReport {
    pub conforms: bool,
    pub results: Vec<RudofResult>,
}
