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
        Self { term_type: "NamedNode".into(), value: v.into(), datatype: None, language: None }
    }
    pub fn blank(v: &str) -> Self {
        Self { term_type: "BlankNode".into(), value: v.into(), datatype: None, language: None }
    }
    pub fn literal(v: &str, datatype: Option<String>, language: Option<String>) -> Self {
        Self { term_type: "Literal".into(), value: v.into(), datatype, language }
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NodeShapeIR {
    pub id: String,
    pub target_classes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_class: Option<String>,
    pub properties: Vec<PropertyShapeIR>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<TermValue>,
    pub message: Vec<String>,
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
