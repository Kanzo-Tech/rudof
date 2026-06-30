#![doc = include_str!("../README.md")]

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// Every rudof-native type the binding marshals against comes from the façade
// (`rudof_lib::form`), so this crate depends on `rudof_lib` alone — it never
// reaches into `shacl`/`rudof_rdf`/`oxrdf` directly.
use rudof_lib::form::{
    BlankNode, FormEngine, Literal, NamedNode, NamedOrBlankNode, RDFFormat, Term as OxTerm,
};

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
    engine: FormEngine,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session {
        Session { engine: FormEngine::new() }
    }

    #[wasm_bindgen(js_name = loadShapes)]
    pub fn load_shapes(&mut self, text: String, media_type: String) -> Result<JsValue, JsError> {
        self.engine
            .load_shapes(&text, &format_of(&media_type))
            .map_err(|e| JsError::new(&e.to_string()))?;
        let ast = self.engine.shapes_ast().expect("shapes just loaded");
        let graph = self.engine.shapes_graph().expect("shapes just loaded");
        let json = shapes::schema_to_json(ast, graph);
        to_js(&json)
    }

    #[wasm_bindgen(js_name = loadData)]
    pub fn load_data(&mut self, text: String, media_type: String) -> Result<(), JsError> {
        self.engine
            .load_data(&text, &format_of(&media_type))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    #[wasm_bindgen(js_name = newData)]
    pub fn new_data(&mut self) {
        self.engine.new_data();
    }

    pub fn add(&mut self, subject: JsValue, predicate: JsValue, object: JsValue) -> Result<(), JsError> {
        let (s, p, o): (TermValue, TermValue, TermValue) = (from_js(subject)?, from_js(predicate)?, from_js(object)?);
        self.engine
            .add_triple(term_to_subject(&s)?, named(&p.value), term_to_object(&o))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn remove(&mut self, subject: JsValue, predicate: JsValue, object: JsValue) -> Result<(), JsError> {
        let (s, p, o): (TermValue, TermValue, TermValue) = (from_js(subject)?, from_js(predicate)?, from_js(object)?);
        self.engine
            .remove_triple(term_to_subject(&s)?, named(&p.value), term_to_object(&o))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn quads(&self, subject: JsValue, predicate: JsValue, object: JsValue) -> Result<JsValue, JsError> {
        let s: Option<TermValue> = from_js(subject)?;
        let p: Option<TermValue> = from_js(predicate)?;
        let o: Option<TermValue> = from_js(object)?;
        let out: Vec<RudofQuad> = self
            .engine
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
        self.engine.serialize(&format_of(&media_type)).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Serialize only the subgraph reachable from `focus` (a `TermValue`) — the
    /// focus-scoped form output, vs whole-graph [`serialize`].
    #[wasm_bindgen(js_name = serializeFocus)]
    pub fn serialize_focus(&self, focus: JsValue, media_type: String) -> Result<String, JsError> {
        let focus: TermValue = from_js(focus)?;
        let focus = term_to_object(&focus);
        self.engine
            .serialize_focus(&focus, &format_of(&media_type))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    #[wasm_bindgen(js_name = projectForm)]
    pub fn project_form(&self, focus: JsValue, shape_id: String) -> Result<JsValue, JsError> {
        let focus: TermValue = from_js(focus)?;
        match self.engine.shapes_ast() {
            Some(ast) => to_js(&project::project_form(&self.engine, ast, &focus, &shape_id)),
            None => to_js(&ProjectedForm { focus, properties: vec![] }),
        }
    }

    /// Validate the current data graph against the loaded shapes (native engine).
    ///
    /// * `shape_id == None`     → validate the whole graph against every shape.
    /// * `shape_id == Some(id)` → validate only that shape (and its nested
    ///   property shapes) against its own targets — shape-scoped.
    pub fn validate(&self, shape_id: Option<String>) -> Result<JsValue, JsError> {
        let outcome = match shape_id {
            Some(id) => self.engine.validate_shape(&id),
            None => self.engine.validate(),
        }
        .map_err(|e| JsError::new(&e.to_string()))?;
        to_js(&validate::report_from_outcome(&outcome))
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
        let focus = validate::focus_object(&focus).map_err(|e| JsError::new(&e))?;
        let outcome = self.engine.validate_focus(&shape_id, &focus).map_err(|e| JsError::new(&e.to_string()))?;
        to_js(&validate::report_from_outcome(&outcome))
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
