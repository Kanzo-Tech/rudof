//! Curated **form session** façade — a wasm-capable surface over rudof's
//! wasm-clean SHACL/RDF stack (`shacl` without the `sparql` feature, `rudof_rdf`,
//! `oxrdf`). It is the single dependency the `rudof_wasm` binding routes its
//! GRAPH / VALIDATE / PROJECT / SERIALIZE operations through, so the binding no
//! longer reaches into `shacl`/`rudof_rdf`/`oxrdf` internals.
//!
//! The native [`crate::Rudof`] façade is endpoint/SPARQL-aware and pulls in
//! `sparql_service` (not part of the wasm build), so this module is a separate,
//! `cfg(target_family = "wasm")` surface that operates directly on an in-memory
//! graph. [`Shapes`] is a parsed shapes graph, validated against through SQL on
//! the host's engine; [`FormEngine`] owns a live data graph under one of them,
//! and exposes:
//!
//! * graph mutation — [`FormEngine::add_triple`] / [`FormEngine::remove_triple`],
//!   [`FormEngine::new_data`], pattern read via [`FormEngine::quads`];
//! * (de)serialization — [`FormEngine::load_data`] / [`FormEngine::serialize`];
//! * SHACL property-path projection — [`FormEngine::eval_path`];
//! * validation — whole-graph [`FormEngine::validate`], shape-scoped
//!   [`FormEngine::validate_shape`] and single-focus [`FormEngine::validate_focus`].
//!
//! Marshalling to/from the JS DTOs (`TermValue`, the form-IR projection) stays in
//! the binding; this façade speaks only rudof-native types, which it re-exports
//! below so consumers depend on `rudof_lib` alone.

// ---- façade prelude: rudof-native types the binding marshals against ---------
pub use oxrdf::{BlankNode, Literal, NamedNode, NamedOrBlankNode, Quad, Term};
pub use prefixmap::IriRef;
pub use rudof_iri::IriS;
pub use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
pub use rudof_rdf::term::Object;
pub use rudof_rdf::term::literal::ConcreteLiteral;
pub use rudof_rdf::{BuildRDF, RDFFormat, SHACLPath};
pub use shacl::algebra::Unchecked;
pub use shacl::ast::{ASTComponent, ASTNodeShape, ASTPropertyShape, ASTSchema, ASTShape};
pub use shacl::types::{NodeKind, Severity, Target, Value};
pub use shacl::validator::report::ValidationResult;
pub use shacl::validator::sql::{RESULT_COLUMNS, Row as SqlRow, SqlEngine};
pub use shacl::vocab::shui;

use rudof_rdf::STRING_BASE;

use oxrdfio::RdfSyntaxError;
use rudof_rdf::backend::OxigraphInMemoryError;
use shacl::ir::{IRSchema, ShapeLabelIdx};
use shacl::messages::MessageCatalog;
use shacl::rdf::ShaclParser;
use shacl::validator::report::ValidationReport;
use std::collections::HashSet;

/// Errors surfaced by the form façade. Flat, `thiserror`-derived (no `Box<dyn>`):
/// the binding renders them to a `JsError` via `Display`.
#[derive(Debug, thiserror::Error)]
pub enum FormError {
    /// The text is not in the syntax of its format: the RDF parser's own error,
    /// placed in the text when the parser can ([`RdfSyntaxError::location`]).
    #[error(transparent)]
    Syntax(RdfSyntaxError),
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Serialize(String),
    #[error("{0}")]
    Graph(String),
    #[error("shape not found in shapes graph: {0}")]
    ShapeNotFound(String),
    #[error("{0}")]
    Validation(String),
}

/// A validation outcome flattened to the shape the form ABI needs: a conformance
/// flag plus the (owned) result list. Whole-graph validation carries the report's
/// own `conforms()`; scoped validation conforms exactly when it produced no
/// results.
pub struct ValidationOutcome {
    pub conforms: bool,
    pub results: Vec<ValidationResult>,
    /// The shapes the engine did not check.
    pub unchecked: Vec<Unchecked>,
}

/// A parsed SHACL shapes graph: the graph itself, kept for annotation reads, its
/// validation AST, and the catalog that words the results of shapes with no
/// `sh:message` of their own.
#[derive(Clone)]
pub struct Shapes {
    graph: OxigraphInMemory,
    ast: ASTSchema,
    /// The built-in catalog until [`Shapes::load_messages`] extends it.
    messages: Option<MessageCatalog>,
}

impl Shapes {
    /// Parse `text` as a SHACL shapes graph, resolving its relative IRIs against
    /// `base` (see [`FormEngine::parse_graph`]).
    pub fn parse(text: &str, format: &RDFFormat, base: Option<&str>) -> Result<Self, FormError> {
        let graph = FormEngine::parse_graph(text, format, base)?;
        let ast = ShaclParser::new(graph.clone())
            .parse()
            .map_err(|e| FormError::Parse(e.to_string()))?;
        Ok(Shapes {
            graph,
            ast,
            messages: None,
        })
    }

    /// Add the `sh:message` literals of `text` to the message catalog that words
    /// the results of shapes with no `sh:message` of their own (SHACL §3.6.2.7).
    /// The built-in catalog (English, Spanish, Catalan) is the starting point; per
    /// constraint component and language, the later document wins. Adding a
    /// language needs no code. Nothing changes when `text` does not parse.
    pub fn load_messages(&mut self, text: &str, format: &RDFFormat) -> Result<(), FormError> {
        let mut catalog = self
            .messages
            .clone()
            .unwrap_or_else(|| MessageCatalog::builtin().clone());
        catalog
            .load(text, format)
            .map_err(|e| FormError::Parse(e.to_string()))?;
        self.messages = Some(catalog);
        Ok(())
    }

    /// The parsed shapes AST (form-IR projection input).
    pub fn ast(&self) -> &ASTSchema {
        &self.ast
    }

    /// The raw shapes graph (presentation/annotation reads).
    pub fn graph(&self) -> &OxigraphInMemory {
        &self.graph
    }

    /// Validate, through SQL on the host's `engine`, the data in the relation
    /// `triples` (columns `s_k, s_v, p, o_k, o_v, o_d, o_l`), with every shape's
    /// focus nodes among the `s_k, s_v` of the relation `focus` when given.
    /// Messages are worded as [`FormEngine::validate`] words them. The shapes are
    /// compiled now; the future owns what it needs, so it outlives this borrow.
    pub fn validate_sql<E: SqlEngine + 'static>(
        &self,
        triples: String,
        focus: Option<String>,
        engine: E,
    ) -> Result<impl std::future::Future<Output = Result<ValidationOutcome, FormError>> + 'static, FormError> {
        let schema = self.compile()?;
        Ok(async move {
            shacl::validator::sql::validate(&schema, &triples, focus.as_deref(), &engine)
                .await
                .map(outcome)
                .map_err(|e| FormError::Validation(e.to_string()))
        })
    }

    /// Write, through SQL on the host's `engine`, the Shape Fragment of the
    /// data in the relation `triples` to the table `into`: its rows that make a
    /// conforming focus node conform, with the focus nodes among the relation
    /// `focus` when given. Resolves to the shapes that have no fragment.
    pub fn fragment_sql<E: SqlEngine + 'static>(
        &self,
        triples: String,
        focus: Option<String>,
        into: String,
        engine: E,
    ) -> Result<impl std::future::Future<Output = Result<Vec<Unchecked>, FormError>> + 'static, FormError> {
        let schema = self.compile()?;
        Ok(async move {
            shacl::validator::sql::fragment(&schema, &triples, focus.as_deref(), &into, &engine)
                .await
                .map_err(|e| FormError::Validation(e.to_string()))
        })
    }

    /// Compile the AST into the validator's internal representation.
    fn compile(&self) -> Result<IRSchema, FormError> {
        let ir = IRSchema::try_from(&self.ast).map_err(|e| FormError::Validation(e.to_string()))?;
        Ok(match &self.messages {
            Some(catalog) => ir.with_messages(catalog.clone()),
            None => ir,
        })
    }
}

/// One form session: a live data graph under a set of [`Shapes`].
pub struct FormEngine {
    data: OxigraphInMemory,
    shapes: Shapes,
}

impl FormEngine {
    /// A session under `shapes`, with an empty data graph.
    pub fn new(shapes: Shapes) -> Self {
        FormEngine {
            data: OxigraphInMemory::new(),
            shapes,
        }
    }

    /// The shapes the session validates and projects against.
    pub fn shapes(&self) -> &Shapes {
        &self.shapes
    }

    /// Parse RDF text into an in-memory graph, resolving relative IRIs against
    /// `base`.
    ///
    /// Deliberately [`ReaderMode::Strict`]: under [`ReaderMode::Lax`] a triple
    /// whose IRI is malformed (Turtle's `IRIREF` production excludes `|`, `<`,
    /// `{`, …) is *dropped* and the parse still reports success, so a document
    /// could be made to conform simply by embedding a bad IRI. A syntax error
    /// has to reach the caller rather than show up as a missing triple.
    ///
    /// Strictness makes the base load-bearing. A **relative** IRI is not a
    /// syntax error — Turtle defines it as a reference resolved against the
    /// document base (RDF 1.2 Turtle §6.3) — so rejecting one is a bug, not a
    /// conformance win. Which is why no loading path in this workspace parses
    /// without a base: `load_data` takes `base: IriS`, not an `Option`, filled
    /// from the caller's `--base-data` / `--base-shapes` or, failing that, from
    /// wherever the document came from (`InputSpec::guess_base`: a `file://`
    /// URL for a path, the endpoint URL for a URL, `stdin://` for stdin).
    ///
    /// This façade follows that convention rather than inventing a third one:
    /// the caller supplies the base when it knows one (the wasm binding takes it
    /// as an optional argument on `loadData` / `Shapes.parse`), and when it does
    /// not, a string has no location to derive a base from, so the parse falls
    /// back to the workspace's own synthetic string base, [`STRING_BASE`].
    pub fn parse_graph(text: &str, format: &RDFFormat, base: Option<&str>) -> Result<OxigraphInMemory, FormError> {
        let base = base.unwrap_or(STRING_BASE);
        OxigraphInMemory::from_str(text, format, Some(base), &ReaderMode::Strict).map_err(|e| match e {
            OxigraphInMemoryError::Syntax { error, .. } => FormError::Syntax(error),
            e => FormError::Parse(e.to_string()),
        })
    }

    // ---- data ----------------------------------------------------------------

    /// Replace the live data graph with the parse of `text`, resolving its
    /// relative IRIs against `base` (see [`FormEngine::parse_graph`]).
    pub fn load_data(&mut self, text: &str, format: &RDFFormat, base: Option<&str>) -> Result<(), FormError> {
        self.data = Self::parse_graph(text, format, base)?;
        Ok(())
    }

    /// Reset the live data graph to empty.
    pub fn new_data(&mut self) {
        self.data = OxigraphInMemory::new();
    }

    /// Add a single triple to the live data graph.
    pub fn add_triple(
        &mut self,
        subject: NamedOrBlankNode,
        predicate: NamedNode,
        object: Term,
    ) -> Result<(), FormError> {
        self.data
            .add_triple(subject, predicate, object)
            .map_err(|e| FormError::Graph(e.to_string()))
    }

    /// Remove a single triple from the live data graph.
    pub fn remove_triple(
        &mut self,
        subject: NamedOrBlankNode,
        predicate: NamedNode,
        object: Term,
    ) -> Result<(), FormError> {
        self.data
            .remove_triple(subject, predicate, object)
            .map_err(|e| FormError::Graph(e.to_string()))
    }

    /// Iterate the live data graph as quads (default graph). The binding applies
    /// its `(s, p, o)` pattern filter and marshals each match.
    pub fn quads(&self) -> impl Iterator<Item = Quad> + '_ {
        self.data.quads()
    }

    /// Serialize the live data graph to a string in `format`.
    pub fn serialize(&self, format: &RDFFormat) -> Result<String, FormError> {
        let mut buf: Vec<u8> = Vec::new();
        BuildRDF::serialize(&self.data, format, &mut buf).map_err(|e| FormError::Serialize(e.to_string()))?;
        String::from_utf8(buf).map_err(|e| FormError::Serialize(e.to_string()))
    }

    /// Serialize only the subgraph reachable from `focus` — its outgoing triples,
    /// recursing through resource (IRI / blank-node) objects — to `format`. This is
    /// the focus-scoped form output: a form edits one subject, so callers want just
    /// that record, not every subject the data graph happens to hold. Prefixes are
    /// copied from the live graph so output stays compact.
    pub fn serialize_focus(&self, focus: &Term, format: &RDFFormat) -> Result<String, FormError> {
        let mut sub = OxigraphInMemory::new();
        sub.merge_prefixes(self.data.prefixmap().clone())
            .map_err(|e| FormError::Graph(e.to_string()))?;

        let mut seen: std::collections::HashSet<NamedOrBlankNode> = std::collections::HashSet::new();
        let mut stack: Vec<Term> = vec![focus.clone()];
        while let Some(node) = stack.pop() {
            let Some(subj) = as_subject(&node) else { continue };
            if !seen.insert(subj.clone()) {
                continue;
            }
            for q in self.data.quads().filter(|q| q.subject == subj) {
                if matches!(q.object, Term::NamedNode(_) | Term::BlankNode(_)) {
                    stack.push(q.object.clone());
                }
                sub.add_triple(q.subject.clone(), q.predicate.clone(), q.object.clone())
                    .map_err(|e| FormError::Graph(e.to_string()))?;
            }
        }

        let mut buf: Vec<u8> = Vec::new();
        BuildRDF::serialize(&sub, format, &mut buf).map_err(|e| FormError::Serialize(e.to_string()))?;
        String::from_utf8(buf).map_err(|e| FormError::Serialize(e.to_string()))
    }

    // ---- projection ----------------------------------------------------------

    /// Evaluate a SHACL property path from `focus` against the live data graph,
    /// yielding the reached terms. Supports the full path grammar (predicate,
    /// inverse, sequence, alternative, the `*`/`+`/`?` closures).
    pub fn eval_path(&self, focus: &Term, path: &SHACLPath) -> Vec<Term> {
        eval_path(&self.data, focus, path)
    }

    // ---- validation ----------------------------------------------------------

    /// Validate the whole live data graph against every loaded shape.
    pub fn validate(&self) -> Result<ValidationOutcome, FormError> {
        let ir = self.compile()?;
        shacl::validator::validate(&ir, &self.data)
            .map(outcome)
            .map_err(|e| FormError::Validation(e.to_string()))
    }

    /// Validate only the shape identified by `shape_id` (and its nested property
    /// shapes) against the data graph — shape-scoped: the shape's own targets are
    /// computed and validated, the rest of the schema is skipped.
    pub fn validate_shape(&self, shape_id: &str) -> Result<ValidationOutcome, FormError> {
        let ir = self.compile()?;
        let idx = resolve_idx(&ir, shape_id)?;
        shacl::validator::validate_shape(&ir, &self.data, idx, None)
            .map(outcome)
            .map_err(|e| FormError::Validation(e.to_string()))
    }

    /// Validate a single `focus` node against the shape identified by `shape_id`
    /// (per-keystroke / per-field scope): no full-graph scan.
    pub fn validate_focus(&self, shape_id: &str, focus: &Object) -> Result<ValidationOutcome, FormError> {
        let ir = self.compile()?;
        let idx = resolve_idx(&ir, shape_id)?;
        shacl::validator::validate_shape(&ir, &self.data, idx, Some(focus))
            .map(outcome)
            .map_err(|e| FormError::Validation(e.to_string()))
    }

    /// Does `focus` conform to `shape`? Resolves the shape by `Object` (IRI OR
    /// blank node), so blank-node `sh:if` condition shapes — which
    /// [`validate_focus`]/[`validate_shape`] cannot address (they resolve IRI
    /// ids only) — can be evaluated. Conformance is decided canonically by the
    /// validator: the focus conforms exactly when scoped validation yields no
    /// results.
    ///
    /// [`validate_focus`]: FormEngine::validate_focus
    /// [`validate_shape`]: FormEngine::validate_shape
    pub fn conforms_focus(&self, shape: &Object, focus: &Object) -> Result<bool, FormError> {
        let ir = self.compile()?;
        let idx = ir
            .get_idx(shape)
            .copied()
            .ok_or_else(|| FormError::ShapeNotFound(format!("{shape}")))?;
        let report = shacl::validator::validate_shape(&ir, &self.data, idx, Some(focus))
            .map_err(|e| FormError::Validation(e.to_string()))?;
        match report.unchecked().iter().find(|u| &u.shape == shape) {
            Some(u) => Err(FormError::Validation(format!("{shape} is not checked: {}", u.reason))),
            None => Ok(report.conforms()),
        }
    }

    /// Compile the shapes into the validator's internal representation.
    fn compile(&self) -> Result<IRSchema, FormError> {
        self.shapes.compile()
    }
}

/// Asks whether a node of one graph conforms to a shape of another: the shapes
/// come from `shapes`, the node from `data`, and the two need not be related.
///
/// [`FormEngine`] cannot say this: it validates its own data graph against its own
/// shapes. SHACL UI needs the split — its matcher shapes live in a *scoring
/// graph* and are run against nodes of the application's *shapes graph* — so the
/// two graphs are parameters here. The shapes are compiled once; each
/// [`NodeChecker::conforms`] is then one scoped validation.
pub struct NodeChecker {
    shapes: IRSchema,
    data: OxigraphInMemory,
    subjects: HashSet<NamedOrBlankNode>,
}

impl NodeChecker {
    /// Compile `shapes` and take `data` as the graph whose nodes are checked.
    pub fn new(shapes: &OxigraphInMemory, data: &OxigraphInMemory) -> Result<Self, FormError> {
        let ast = ShaclParser::new(shapes.clone())
            .parse()
            .map_err(|e| FormError::Parse(e.to_string()))?;
        let shapes = IRSchema::try_from(&ast).map_err(|e| FormError::Validation(e.to_string()))?;
        Ok(Self {
            shapes,
            subjects: data.quads().map(|q| q.subject).collect(),
            data: data.clone(),
        })
    }

    /// Does `focus` conform to `shape` — an IRI or blank node of the compiled
    /// shapes graph?
    ///
    /// A node that is neither a literal nor a subject of `data` does not conform:
    /// this is the SHACL UI *validation function*, which has that step before the
    /// standard validation. A shape the shapes graph does not define is a
    /// [`FormError::ShapeNotFound`].
    pub fn conforms(&self, shape: &Object, focus: &Object) -> Result<bool, FormError> {
        let known = match Term::from(focus.clone()) {
            Term::Literal(_) => true,
            node => as_subject(&node).is_some_and(|s| self.subjects.contains(&s)),
        };
        if !known {
            return Ok(false);
        }
        let idx = self
            .shapes
            .get_idx(shape)
            .copied()
            .ok_or_else(|| FormError::ShapeNotFound(format!("{shape}")))?;
        shacl::validator::validate_shape(&self.shapes, &self.data, idx, Some(focus))
            .map(|report| report.conforms())
            .map_err(|e| FormError::Validation(e.to_string()))
    }
}

/// Resolve a shape's id — an IRI, or `_:label` for an anonymous shape — to its
/// arena index in the compiled schema.
fn resolve_idx(ir: &IRSchema, shape_id: &str) -> Result<ShapeLabelIdx, FormError> {
    let shape_ref = match shape_id.strip_prefix("_:") {
        Some(label) => Object::bnode(label.to_string()),
        None => Object::iri(IriS::new_unchecked(shape_id)),
    };
    ir.get_idx(&shape_ref)
        .copied()
        .ok_or_else(|| FormError::ShapeNotFound(shape_id.to_string()))
}

/// A scoped result list conforms exactly when it is empty (matching the report's
/// own `conforms()`).
fn outcome(report: ValidationReport) -> ValidationOutcome {
    ValidationOutcome {
        conforms: report.conforms(),
        results: report.results().clone(),
        unchecked: report.unchecked().to_vec(),
    }
}

// ---- SHACL property-path evaluation -----------------------------------------

fn eval_path(graph: &OxigraphInMemory, node: &Term, path: &SHACLPath) -> Vec<Term> {
    match path {
        SHACLPath::Predicate { pred } => {
            let Some(subj) = as_subject(node) else { return vec![] };
            graph
                .quads()
                .filter(|q| q.subject == subj && q.predicate.as_str() == pred.as_str())
                .map(|q| q.object.clone())
                .collect()
        },
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
        },
        SHACLPath::Alternative { paths } => paths.iter().flat_map(|p| eval_path(graph, node, p)).collect(),
        SHACLPath::ZeroOrMore { path } => closure(graph, node, path, true),
        SHACLPath::OneOrMore { path } => closure(graph, node, path, false),
        SHACLPath::ZeroOrOne { path } => {
            let mut v = vec![node.clone()];
            v.extend(eval_path(graph, node, path));
            v
        },
    }
}

fn closure(graph: &OxigraphInMemory, start: &Term, step: &SHACLPath, include_start: bool) -> Vec<Term> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut stack: Vec<Term> = if include_start {
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

fn as_subject(t: &Term) -> Option<NamedOrBlankNode> {
    match t {
        Term::NamedNode(n) => Some(NamedOrBlankNode::NamedNode(n.clone())),
        Term::BlankNode(b) => Some(NamedOrBlankNode::BlankNode(b.clone())),
        _ => None,
    }
}

fn subject_to_term(s: &NamedOrBlankNode) -> Term {
    match s {
        NamedOrBlankNode::NamedNode(n) => Term::NamedNode(n.clone()),
        NamedOrBlankNode::BlankNode(b) => Term::BlankNode(b.clone()),
    }
}
