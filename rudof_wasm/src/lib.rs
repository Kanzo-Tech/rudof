#![doc = include_str!("../README.md")]

use serde::de::DeserializeOwned;
use serde::Serialize;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

// Every rudof-native type the binding marshals against comes from the façade
// (`rudof_lib::form`), so this crate depends on `rudof_lib` alone — it never
// reaches into `shacl`/`rudof_rdf`/`oxrdf` directly.
use rudof_lib::form::{
    BlankNode, FormEngine, FormError, Literal, NamedNode, NamedOrBlankNode, RDFFormat, Term as OxTerm,
};

mod dto;
mod index;
mod project;
mod scoring;
mod shapes;
mod sql;
mod validate;
use dto::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// A DTO as the JavaScript value its TypeScript declaration describes.
fn to_js<T: Tsify + Serialize>(v: &T) -> Result<Ts<T>, JsError> {
    Ts::from_rust(v).map_err(|e| JsError::new(&e.to_string()))
}
/// A JavaScript value as the DTO its TypeScript declaration names.
fn from_js<T: Tsify + DeserializeOwned>(v: Ts<T>) -> Result<T, JsError>
where
    T::JsType: Clone,
{
    v.to_rust().map_err(|e| JsError::new(&e.to_string()))
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
            let dt = if lang.is_none() {
                Some(l.datatype().as_str().to_string())
            } else {
                None
            };
            TermValue::literal(l.value(), dt, lang)
        },
        // RDF-star quoted triple — not modelled in the form IR.
        _ => TermValue::blank("rdfstar"),
    }
}

// ---- TypeScript declarations the DTOs cannot carry -----------------------------

#[wasm_bindgen(typescript_custom_section)]
const TS_TYPES: &'static str = r#"
/**
 * The page's SQL engine: `@fossil-lang/types`' `Engine`, as far as rudof
 * reads it, which `@kanzo-tech/mosaic`'s `engine()` is.
 */
export interface Engine {
  query(sql: string, options: { readonly signal: AbortSignal }): Promise<Table>;
}

/**
 * An answer, in columns: `@fossil-lang/types`' `Table`, the part of an Arrow
 * table a reader reads. rudof reads each column by name with `getChild`.
 */
export interface Table {
  readonly numRows: number;
  readonly schema: { readonly fields: readonly { readonly name: string }[] };
  /** One column by name, or `null` when the answer has none of that name. */
  getChild(name: string): Column | null;
}

/** One column of a `Table`: `get` answers `null` for a null. */
export interface Column {
  readonly length: number;
  get(index: number): unknown;
  toArray(): ArrayLike<unknown>;
}

/**
 * What `Shapes.parse` throws. When the RDF parser places the syntax error,
 * `line` and `column` are where it starts, from 1 as the parser's own message
 * counts them, `column` in code points; an error it does not place (a SHACL
 * error in a graph that parsed, an invalid IRI) has neither.
 */
export interface ShapesError extends Error {
  line?: number;
  column?: number;
}

/** What `Shapes.validate` reads: a triples relation, on an engine. */
export interface TableValidation {
  /** A relation of columns `s_type, s_value, p, o_type, o_value, o_datatype, o_lang`, e.g. `"job".triples`. */
  table: string;
  /**
   * A relation of nodes in columns `s_type, s_value`, spelled as the table's
   * subjects: each shape's focus nodes are its targets among them, checked
   * against the whole table. A selection, for one.
   */
  focus?: string;
  engine: Engine;
  /**
   * Stops the running statement. Every statement carries it, as the engine's
   * `query` takes one: pass a signal that never aborts when nothing stops it.
   */
  signal: AbortSignal;
}

/** What `Shapes.fragment` reads: a `TableValidation`, and the table it writes. */
export interface TableFragment extends TableValidation {
  /** The table the fragment is written to, replaced when it exists, e.g. `"job".fragment`. */
  into: string;
}
"#;

// ---- Shapes ------------------------------------------------------------------

/// A parsed SHACL shapes graph. It validates a triples relation on the page's
/// engine, and a `FormSession` edits data under it.
#[wasm_bindgen]
pub struct Shapes {
    inner: rudof_lib::form::Shapes,
}

#[wasm_bindgen]
impl Shapes {
    /// Parse a SHACL shapes graph. `options.mediaType` defaults to Turtle;
    /// `options.base` is the document base relative IRIs resolve against — the
    /// URL the shapes were fetched from, when the caller knows it. Omitted, the
    /// parse falls back to the workspace's synthetic string base; see
    /// [`FormEngine::parse_graph`]. Throws a `ShapesError`.
    pub fn parse(text: String, options: Option<Ts<ParseOptions>>) -> Result<Shapes, JsValue> {
        let ParseOptions { media_type, base } = options.map(from_js).transpose()?.unwrap_or_default();
        let inner = rudof_lib::form::Shapes::parse(
            &text,
            &format_of(media_type.as_deref().unwrap_or("text/turtle")),
            base.as_deref(),
        )
        .map_err(shapes_error)?;
        Ok(Shapes { inner })
    }

    /// The shapes as the vocabulary-agnostic model a form renders.
    pub fn model(&self) -> Result<Ts<ShapeModelJson>, JsError> {
        to_js(&shapes::schema_to_json(self.inner.ast(), self.inner.graph()))
    }

    /// Add default validation messages: `sh:message` literals on constraint
    /// components (`sh:MinCountConstraintComponent sh:message "..."@fr`, with
    /// `{$minCount}`-style placeholders), used for results whose shape has no
    /// `sh:message`. They extend the built-in English, Spanish and Catalan
    /// messages; per component and language the later document wins. `mediaType`
    /// defaults to Turtle. A `FormSession` words its results with the messages
    /// its shapes had when it was made.
    #[wasm_bindgen(js_name = loadMessages)]
    pub fn load_messages(&mut self, text: String, media_type: Option<String>) -> Result<(), JsError> {
        self.inner
            .load_messages(&text, &format_of(media_type.as_deref().unwrap_or("text/turtle")))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Validate, through SQL on the page's engine, the data in a triples
    /// relation: `table` names it (columns `s_type, s_value, p, o_type, o_value, o_datatype, o_lang`,
    /// e.g. `"job".triples`, the view `@fossil-lang/corpus`'s `open` creates);
    /// `engine` is `{ query(sql, { signal }): Promise<Table> }`, which
    /// `@kanzo-tech/mosaic`'s `engine()` is; `signal` stops the running
    /// statement. Every statement runs through `engine.query`, on its one
    /// connection.
    ///
    /// Resolves to a `ValidationReport`, worded as `FormSession.validate` words it.
    /// A shape the engine does not check (outside its profile, recursive, or
    /// depending on such a shape) is listed in `unchecked`; the rest are
    /// checked.
    #[wasm_bindgen(unchecked_return_type = "Promise<ValidationReport>")]
    pub fn validate(
        &self,
        #[wasm_bindgen(unchecked_param_type = "TableValidation")] options: JsValue,
    ) -> Result<js_sys::Promise, JsError> {
        let (table, focus, engine) = table_options(&options)?;
        let validation = self
            .inner
            .validate_sql(table, focus, engine)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let outcome = validation.await.map_err(|e| JsError::new(&e.to_string()))?;
            Ok(to_js(&validate::report_from_outcome(&outcome))?.into())
        }))
    }

    /// Write the Shape Fragment of a triples relation to the table `into`
    /// (replacing it): the rows of `table`, in its seven columns, that make a
    /// conforming focus node conform to its shapes, with the focus nodes among
    /// `focus` when given; the subset of the data the shapes describe. Options
    /// are `validate`'s, plus `into`.
    ///
    /// Resolves to `{ unchecked }`: the shapes outside the fragments profile,
    /// which the fragment does not cover.
    #[wasm_bindgen(unchecked_return_type = "Promise<RudofFragment>")]
    pub fn fragment(
        &self,
        #[wasm_bindgen(unchecked_param_type = "TableFragment")] options: JsValue,
    ) -> Result<js_sys::Promise, JsError> {
        let (table, focus, engine) = table_options(&options)?;
        let into = js_sys::Reflect::get(&options, &"into".into())
            .ok()
            .and_then(|v| v.as_string())
            .ok_or_else(|| JsError::new("into: expected the name of the table to write"))?;
        let fragment = self
            .inner
            .fragment_sql(table, focus, into, engine)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let unchecked = fragment.await.map_err(|e| JsError::new(&e.to_string()))?;
            Ok(to_js(&RudofFragment {
                unchecked: validate::unchecked_to_dto(&unchecked),
            })?
            .into())
        }))
    }
}

/// `error` as the `ShapesError` its TypeScript declares: an `Error` carrying,
/// when the parser places it, the start of `RdfSyntaxError::location` (0-based)
/// as the 1-based `line` and `column` the parser's message prints.
fn shapes_error(error: FormError) -> JsValue {
    let thrown = js_sys::Error::new(&error.to_string());
    let start = match &error {
        FormError::Syntax(e) => e.location().map(|at| at.start),
        _ => None,
    };
    for (name, at) in start.into_iter().flat_map(|p| [("line", p.line), ("column", p.column)]) {
        js_sys::Reflect::set(&thrown, &name.into(), &JsValue::from_f64((at + 1) as f64))
            .expect("an Error takes properties");
    }
    thrown.into()
}

/// The triples relation, the focus relation and the engine of a
/// `TableValidation`.
fn table_options(options: &JsValue) -> Result<(String, Option<String>, sql::JsEngine), JsError> {
    let field = |name: &str| js_sys::Reflect::get(options, &name.into()).unwrap_or(JsValue::UNDEFINED);
    let table = field("table")
        .as_string()
        .ok_or_else(|| JsError::new("table: expected the name of a triples relation"))?;
    let focus = field("focus");
    let focus = match focus.is_undefined() || focus.is_null() {
        true => None,
        false => Some(
            focus
                .as_string()
                .ok_or_else(|| JsError::new("focus: expected the name of a relation of nodes"))?,
        ),
    };
    let engine = sql::JsEngine::new(field("engine"), field("signal")).map_err(|e| JsError::new(&e))?;
    Ok((table, focus, engine))
}

// ---- The form session --------------------------------------------------------

/// One form session: a live data graph under a set of `Shapes`, edited,
/// projected and validated in memory.
#[wasm_bindgen]
pub struct FormSession {
    engine: FormEngine,
}

#[wasm_bindgen]
impl FormSession {
    /// A session under `shapes`, with an empty data graph. It keeps the shapes
    /// as they are now: messages loaded into them later do not reach it.
    #[wasm_bindgen(constructor)]
    pub fn new(shapes: &Shapes) -> FormSession {
        FormSession {
            engine: FormEngine::new(shapes.inner.clone()),
        }
    }

    /// Replace the live data graph with the parse of `text`.
    ///
    /// `base` is the document base relative IRIs in `text` resolve against — the
    /// URL the document was fetched from, when the caller knows it. Omitted
    /// (`undefined`/`null`), the parse falls back to the workspace's synthetic
    /// string base; see [`FormEngine::parse_graph`].
    #[wasm_bindgen(js_name = loadData)]
    pub fn load_data(&mut self, text: String, media_type: String, base: Option<String>) -> Result<(), JsError> {
        self.engine
            .load_data(&text, &format_of(&media_type), base.as_deref())
            .map_err(|e| JsError::new(&e.to_string()))
    }

    #[wasm_bindgen(js_name = newData)]
    pub fn new_data(&mut self) {
        self.engine.new_data();
    }

    pub fn add(
        &mut self,
        subject: Ts<TermValue>,
        predicate: Ts<TermValue>,
        object: Ts<TermValue>,
    ) -> Result<(), JsError> {
        let (s, p, o): (TermValue, TermValue, TermValue) = (from_js(subject)?, from_js(predicate)?, from_js(object)?);
        self.engine
            .add_triple(term_to_subject(&s)?, named(&p.value), term_to_object(&o))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn remove(
        &mut self,
        subject: Ts<TermValue>,
        predicate: Ts<TermValue>,
        object: Ts<TermValue>,
    ) -> Result<(), JsError> {
        let (s, p, o): (TermValue, TermValue, TermValue) = (from_js(subject)?, from_js(predicate)?, from_js(object)?);
        self.engine
            .remove_triple(term_to_subject(&s)?, named(&p.value), term_to_object(&o))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// The triples matching a pattern; `null` matches anything.
    pub fn quads(
        &self,
        subject: Option<Ts<TermValue>>,
        predicate: Option<Ts<TermValue>>,
        object: Option<Ts<TermValue>>,
    ) -> Result<Vec<Ts<RudofQuad>>, JsError> {
        let s = subject.map(from_js).transpose()?;
        let p = predicate.map(from_js).transpose()?;
        let o = object.map(from_js).transpose()?;
        self.engine
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
            .map(|q| to_js(&q))
            .collect()
    }

    pub fn serialize(&self, media_type: String) -> Result<String, JsError> {
        self.engine
            .serialize(&format_of(&media_type))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Serialize only the subgraph reachable from `focus` — the focus-scoped
    /// form output, vs whole-graph [`serialize`].
    #[wasm_bindgen(js_name = serializeFocus)]
    pub fn serialize_focus(&self, focus: Ts<TermValue>, media_type: String) -> Result<String, JsError> {
        let focus: TermValue = from_js(focus)?;
        let focus = term_to_object(&focus);
        self.engine
            .serialize_focus(&focus, &format_of(&media_type))
            .map_err(|e| JsError::new(&e.to_string()))
    }

    #[wasm_bindgen(js_name = projectForm)]
    pub fn project_form(&self, focus: Ts<TermValue>, shape_id: String) -> Result<Ts<ProjectedForm>, JsError> {
        let focus: TermValue = from_js(focus)?;
        to_js(&project::project_form(
            &self.engine,
            self.engine.shapes().ast(),
            &focus,
            &shape_id,
        ))
    }

    /// Validate the current data graph against the shapes (in memory).
    ///
    /// * `shape_id == None`     → validate the whole graph against every shape.
    /// * `shape_id == Some(id)` → validate only that shape (and its nested
    ///   property shapes) against its own targets — shape-scoped.
    pub fn validate(&self, shape_id: Option<String>) -> Result<Ts<ValidationReport>, JsError> {
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
    #[wasm_bindgen(js_name = validateFocus)]
    pub fn validate_focus(&self, focus: Ts<TermValue>, shape_id: String) -> Result<Ts<ValidationReport>, JsError> {
        let focus: TermValue = from_js(focus)?;
        let focus = validate::focus_object(&focus).map_err(|e| JsError::new(&e))?;
        let outcome = self
            .engine
            .validate_focus(&shape_id, &focus)
            .map_err(|e| JsError::new(&e.to_string()))?;
        to_js(&validate::report_from_outcome(&outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// A Turtle syntax error throws a `ShapesError` at the parser's line and
    /// column, counted from 1; a SHACL error in a graph that parsed has none.
    #[wasm_bindgen_test]
    fn a_syntax_error_throws_where_the_parser_found_it() {
        let field = |e: &JsValue, name: &str| js_sys::Reflect::get(e, &name.into()).expect("a property");
        let Err(e) = Shapes::parse("prefix : <http://example.org/>\n:s a :T ;\n  :p .\n".into(), None) else {
            panic!("a predicate with no object is not Turtle")
        };
        assert!(e.is_instance_of::<js_sys::Error>());
        assert_eq!(
            (field(&e, "line").as_f64(), field(&e, "column").as_f64()),
            (Some(3.0), Some(6.0))
        );
        assert!(field(&e, "message")
            .as_string()
            .is_some_and(|m| m.starts_with("Parser error at line 3 column 6")));

        let shacl =
            "prefix sh: <http://www.w3.org/ns/shacl#>\n<http://example.org/S> a sh:NodeShape ; sh:minCount \"x\" .\n";
        let Err(e) = Shapes::parse(shacl.into(), None) else {
            panic!("sh:minCount takes an integer")
        };
        assert!(field(&e, "line").is_undefined() && field(&e, "column").is_undefined());
    }

    /// A session under no shapes at all: these tests are about reading data.
    fn empty() -> FormSession {
        FormSession::new(&Shapes::parse(String::new(), None).expect("an empty shapes graph"))
    }

    /// Turtle's `IRIREF` production excludes `|`, so `<http://example.org/bad|iri>`
    /// is a syntax error. Under the reader's lax mode the offending triple was
    /// simply dropped and `loadData` still reported success — a document could
    /// then be made to conform by embedding a malformed IRI, because the triple
    /// that would have violated a constraint was no longer in the graph. A
    /// malformed IRI must surface as an error, not as an absence.
    #[wasm_bindgen_test]
    fn a_malformed_iri_is_a_parse_error_not_a_silently_dropped_triple() {
        const DOC: &str = r#"@prefix ex: <http://example.org/> .
ex:a a ex:T ; ex:p <http://example.org/bad|iri> .
ex:a ex:q "ok" .
"#;

        let mut session = empty();
        let outcome = session.load_data(DOC.to_string(), "text/turtle".to_string(), None);
        assert!(
            outcome.is_err(),
            "loadData accepted a document whose IRI violates Turtle's IRIREF production"
        );

        // The same document with the `|` removed is valid and still loads.
        let mut session = empty();
        session
            .load_data(DOC.replace('|', "-"), "text/turtle".to_string(), None)
            .expect("a well-formed document must still load");
    }

    /// `#` passes Turtle's `IRIREF` character class but a second one does not
    /// survive IRI parsing: RFC 3987 allows exactly one fragment separator. So
    /// supplying a base must not launder this into a success — resolving a
    /// *relative* reference is the only thing the base is there to do.
    #[wasm_bindgen_test]
    fn a_double_fragment_iri_stays_a_parse_error_even_with_a_base() {
        const DOC: &str = r#"@prefix ex: <http://example.org/> .
ex:a ex:p <http://example.org/thing#a#b> .
"#;

        for base in [None, Some("http://example.org/doc".to_string())] {
            let mut session = empty();
            assert!(
                session
                    .load_data(DOC.to_string(), "text/turtle".to_string(), base)
                    .is_err(),
                "loadData accepted an IRI carrying two fragment separators"
            );
        }
    }

    /// A **relative** IRI is not malformed: Turtle resolves it against the
    /// document base. Strict reading with `base: None` turned every such
    /// document into "No scheme found in an absolute IRI" — rejecting input RDF
    /// calls legal. With a base threaded through, the triple loads and its IRIs
    /// come out absolute.
    #[wasm_bindgen_test]
    fn a_relative_iri_resolves_against_the_caller_supplied_base() {
        const DOC: &str = r#"@prefix ex: <http://example.org/> .
<person/1> ex:knows <person/2> .
"#;

        let mut session = empty();
        session
            .load_data(
                DOC.to_string(),
                "text/turtle".to_string(),
                Some("http://example.org/dir/doc.ttl".to_string()),
            )
            .expect("a relative IRI is legal Turtle and must load");

        let quads: Vec<RudofQuad> = session
            .quads(None, None, None)
            .expect("quads")
            .into_iter()
            .map(|q| from_js(q).expect("quad decodes"))
            .collect();

        assert_eq!(quads.len(), 1, "the relative-IRI triple must be in the graph");
        assert_eq!(quads[0].subject.value, "http://example.org/dir/person/1");
        assert_eq!(quads[0].object.value, "http://example.org/dir/person/2");
    }

    /// With no base from the caller the parse still has to succeed: the façade
    /// falls back to the workspace's synthetic string base (the same answer
    /// `InputSpec::guess_base` gives RDF that arrived as a string), so the IRIs
    /// resolve under `string://` rather than the document being rejected.
    #[wasm_bindgen_test]
    fn a_relative_iri_resolves_against_the_synthetic_base_when_none_is_given() {
        const DOC: &str = r#"@prefix ex: <http://example.org/> .
<person/1> ex:knows <person/2> .
"#;

        let mut session = empty();
        session
            .load_data(DOC.to_string(), "text/turtle".to_string(), None)
            .expect("a relative IRI must load even with no caller base");

        let quads: Vec<RudofQuad> = session
            .quads(None, None, None)
            .expect("quads")
            .into_iter()
            .map(|q| from_js(q).expect("quad decodes"))
            .collect();

        assert_eq!(quads.len(), 1, "the relative-IRI triple must be in the graph");
        assert!(
            quads[0].subject.value.starts_with("string:"),
            "expected the synthetic string base, got {}",
            quads[0].subject.value
        );
    }
}
