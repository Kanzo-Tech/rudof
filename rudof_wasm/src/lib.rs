#![doc = include_str!("../README.md")]

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use oxrdf::{BlankNode, Literal, NamedNode, NamedOrBlankNode, Term as OxTerm};
use rudof_rdf::rdf_core::{BuildRDF, RDFFormat};
use rudof_rdf::rdf_impl::{OxigraphInMemory, ReaderMode};
use shacl::ast::ASTSchema;
use shacl::rdf::ShaclParser;

mod dto;
mod project;
mod shapes;
mod validate;
use dto::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

fn to_js<T: Serialize>(v: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(v).map_err(|e| JsError::new(&e.to_string()))
}
fn from_js<T: DeserializeOwned>(v: JsValue) -> Result<T, JsError> {
    serde_wasm_bindgen::from_value(v).map_err(|e| JsError::new(&e.to_string()))
}

fn format_of(media_type: &str) -> RDFFormat {
    match media_type {
        "application/ld+json" | "application/json" => RDFFormat::JsonLd,
        "application/n-triples" | "text/plain" => RDFFormat::NTriples,
        _ => RDFFormat::Turtle,
    }
}

// ---- TermValue <-> oxrdf -----------------------------------------------------

fn named(value: &str) -> NamedNode {
    NamedNode::new_unchecked(value)
}

fn term_to_subject(t: &TermValue) -> Result<NamedOrBlankNode, JsError> {
    match t.term_type.as_str() {
        "NamedNode" => Ok(NamedOrBlankNode::NamedNode(named(&t.value))),
        "BlankNode" => Ok(NamedOrBlankNode::BlankNode(BlankNode::new_unchecked(&t.value))),
        _ => Err(JsError::new("subject must be a NamedNode or BlankNode")),
    }
}

pub(crate) fn term_to_object(t: &TermValue) -> OxTerm {
    match t.term_type.as_str() {
        "NamedNode" => OxTerm::NamedNode(named(&t.value)),
        "BlankNode" => OxTerm::BlankNode(BlankNode::new_unchecked(&t.value)),
        _ => OxTerm::Literal(literal_of(t)),
    }
}

fn literal_of(t: &TermValue) -> Literal {
    if let Some(lang) = &t.language {
        Literal::new_language_tagged_literal_unchecked(&t.value, lang)
    } else if let Some(dt) = &t.datatype {
        Literal::new_typed_literal(&t.value, named(dt))
    } else {
        Literal::new_simple_literal(&t.value)
    }
}

fn subject_to_value(s: &NamedOrBlankNode) -> TermValue {
    match s {
        NamedOrBlankNode::NamedNode(n) => TermValue::named(n.as_str()),
        NamedOrBlankNode::BlankNode(b) => TermValue::blank(b.as_str()),
    }
}

pub(crate) fn object_to_value(o: &OxTerm) -> TermValue {
    match o {
        OxTerm::NamedNode(n) => TermValue::named(n.as_str()),
        OxTerm::BlankNode(b) => TermValue::blank(b.as_str()),
        OxTerm::Literal(l) => {
            let lang = l.language().map(|s| s.to_string());
            let dt = if lang.is_none() { Some(l.datatype().as_str().to_string()) } else { None };
            TermValue::literal(l.value(), dt, lang)
        }
        // RDF-star quoted triple — not modelled in the form IR.
        _ => TermValue::blank("rdfstar"),
    }
}

// ---- The session -------------------------------------------------------------

/// One form session: the live data graph plus the source of the loaded shapes.
#[wasm_bindgen]
pub struct Session {
    data: OxigraphInMemory,
    shapes_graph: Option<OxigraphInMemory>,
    shapes_ast: Option<ASTSchema>,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session {
        Session { data: OxigraphInMemory::new(), shapes_graph: None, shapes_ast: None }
    }

    #[wasm_bindgen(js_name = loadShapes)]
    pub fn load_shapes(&mut self, text: String, media_type: String) -> Result<JsValue, JsError> {
        let graph = OxigraphInMemory::from_str(&text, &format_of(&media_type), None, &ReaderMode::Lax)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let schema = ShaclParser::new(graph.clone())
            .parse()
            .map_err(|e| JsError::new(&e.to_string()))?;
        let json = shapes::schema_to_json(&schema, &graph);
        self.shapes_graph = Some(graph);
        self.shapes_ast = Some(schema);
        to_js(&json)
    }

    #[wasm_bindgen(js_name = loadData)]
    pub fn load_data(&mut self, text: String, media_type: String) -> Result<(), JsError> {
        self.data = OxigraphInMemory::from_str(&text, &format_of(&media_type), None, &ReaderMode::Lax)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = newData)]
    pub fn new_data(&mut self) {
        self.data = OxigraphInMemory::new();
    }

    pub fn add(&mut self, subject: JsValue, predicate: JsValue, object: JsValue) -> Result<(), JsError> {
        let (s, p, o): (TermValue, TermValue, TermValue) = (from_js(subject)?, from_js(predicate)?, from_js(object)?);
        self.data
            .add_triple(term_to_subject(&s)?, named(&p.value), term_to_object(&o))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn remove(&mut self, subject: JsValue, predicate: JsValue, object: JsValue) -> Result<(), JsError> {
        let (s, p, o): (TermValue, TermValue, TermValue) = (from_js(subject)?, from_js(predicate)?, from_js(object)?);
        self.data
            .remove_triple(term_to_subject(&s)?, named(&p.value), term_to_object(&o))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn quads(&self, subject: JsValue, predicate: JsValue, object: JsValue) -> Result<JsValue, JsError> {
        let s: Option<TermValue> = from_js(subject)?;
        let p: Option<TermValue> = from_js(predicate)?;
        let o: Option<TermValue> = from_js(object)?;
        let out: Vec<RudofQuad> = self
            .data
            .quads()
            .filter(|q| {
                s.as_ref().is_none_or(|t| &subject_to_value(&q.subject) == t)
                    && p.as_ref().is_none_or(|t| t.value == q.predicate.as_str())
                    && o.as_ref().is_none_or(|t| &object_to_value(&q.object) == t)
            })
            .map(|q| RudofQuad {
                subject: subject_to_value(&q.subject),
                predicate: TermValue::named(q.predicate.as_str()),
                object: object_to_value(&q.object),
            })
            .collect();
        to_js(&out)
    }

    pub fn serialize(&self, media_type: String) -> Result<String, JsError> {
        let mut buf: Vec<u8> = Vec::new();
        BuildRDF::serialize(&self.data, &format_of(&media_type), &mut buf)
            .map_err(|e| JsError::new(&e.to_string()))?;
        String::from_utf8(buf).map_err(|e| JsError::new(&e.to_string()))
    }

    #[wasm_bindgen(js_name = projectForm)]
    pub fn project_form(&self, focus: JsValue, shape_id: String) -> Result<JsValue, JsError> {
        let focus: TermValue = from_js(focus)?;
        match &self.shapes_ast {
            Some(ast) => to_js(&project::project_form(ast, &self.data, &focus, &shape_id)),
            None => to_js(&ProjectedForm { focus, properties: vec![] }),
        }
    }

    /// Validate the current data graph against the loaded shapes (native engine).
    ///
    /// * `shape_id == None`     → validate the whole graph against every shape.
    /// * `shape_id == Some(id)` → validate only that shape (and its nested
    ///   property shapes) against its own targets — shape-scoped.
    pub fn validate(&self, shape_id: Option<String>) -> Result<JsValue, JsError> {
        let ast = self
            .shapes_ast
            .as_ref()
            .ok_or_else(|| JsError::new("no shapes loaded; call loadShapes first"))?;
        let report = match shape_id {
            Some(id) => validate::validate_shape(&self.data, ast, &id),
            None => validate::validate(&self.data, ast),
        }
        .map_err(|e| JsError::new(&e))?;
        to_js(&report)
    }

    /// Validate a single focus node against a single shape (scoped). This is the
    /// per-field / per-keystroke revalidation path used by the React form: it
    /// validates just `focus` against `shape_id`, not the whole graph.
    ///
    /// `focus` is a `TermValue` (the same `{ termType, value, datatype?,
    /// language? }` shape used everywhere else on this ABI), normally a
    /// `NamedNode`/`BlankNode` resource.
    #[wasm_bindgen(js_name = validateFocus)]
    pub fn validate_focus(&self, focus: JsValue, shape_id: String) -> Result<JsValue, JsError> {
        let focus: TermValue = from_js(focus)?;
        let ast = self
            .shapes_ast
            .as_ref()
            .ok_or_else(|| JsError::new("no shapes loaded; call loadShapes first"))?;
        let report = validate::validate_focus(&self.data, ast, &shape_id, &focus).map_err(|e| JsError::new(&e))?;
        to_js(&report)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
