//! Curated **form session** façade — a wasm-capable surface over rudof's
//! wasm-clean SHACL/RDF stack (`shacl` without the `sparql` feature, `rudof_rdf`,
//! `oxrdf`). It is the single dependency the `rudof_wasm` binding routes its
//! GRAPH / VALIDATE / PROJECT / SERIALIZE operations through, so the binding no
//! longer reaches into `shacl`/`rudof_rdf`/`oxrdf` internals.
//!
//! The native [`crate::Rudof`] façade is endpoint/SPARQL-aware and pulls in
//! `sparql_service` (not part of the wasm build), so this module is a separate,
//! `cfg(target_family = "wasm")` surface that operates directly on an in-memory
//! graph. It owns the live data graph plus the loaded shapes (graph + parsed
//! AST), and exposes:
//!
//! * graph mutation — [`FormEngine::add_triple`] / [`FormEngine::remove_triple`],
//!   [`FormEngine::new_data`], pattern read via [`FormEngine::quads`];
//! * (de)serialization — [`FormEngine::load_data`] / [`FormEngine::load_shapes`]
//!   / [`FormEngine::serialize`];
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
pub use shacl::ast::{ASTComponent, ASTNodeShape, ASTPropertyShape, ASTSchema, ASTShape};
pub use shacl::types::{NodeKind, Severity, Target, Value};
pub use shacl::validator::report::ValidationResult;
pub use shacl::vocab::shui;

use shacl::ir::{IRSchema, ShapeLabelIdx};
use shacl::rdf::ShaclParser;
use shacl::validator::ShaclValidationMode;
use shacl::validator::processor::{GraphValidation, ShaclProcessor};
use shacl::validator::store::Graph;

/// Errors surfaced by the form façade. Flat, `thiserror`-derived (no `Box<dyn>`):
/// the binding renders them to a `JsError` via `Display`.
#[derive(Debug, thiserror::Error)]
pub enum FormError {
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Serialize(String),
    #[error("{0}")]
    Graph(String),
    #[error("no shapes loaded; call loadShapes first")]
    NoShapes,
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
}

/// One form session: the live data graph plus the source of the loaded shapes
/// (the shapes graph, kept for annotation reads, and the parsed validation AST).
#[derive(Default)]
pub struct FormEngine {
    data: OxigraphInMemory,
    shapes_graph: Option<OxigraphInMemory>,
    shapes_ast: Option<ASTSchema>,
}

impl FormEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse RDF text into an in-memory graph (lenient reader).
    pub fn parse_graph(text: &str, format: &RDFFormat) -> Result<OxigraphInMemory, FormError> {
        OxigraphInMemory::from_str(text, format, None, &ReaderMode::Lax).map_err(|e| FormError::Parse(e.to_string()))
    }

    // ---- shapes --------------------------------------------------------------

    /// Parse `text` as a SHACL shapes graph and load it: stores both the raw
    /// graph (annotation reads) and the parsed validation AST. Returns the AST so
    /// the binding can project its form-IR JSON without re-parsing.
    pub fn load_shapes(&mut self, text: &str, format: &RDFFormat) -> Result<&ASTSchema, FormError> {
        let graph = Self::parse_graph(text, format)?;
        let schema = ShaclParser::new(graph.clone())
            .parse()
            .map_err(|e| FormError::Parse(e.to_string()))?;
        self.shapes_graph = Some(graph);
        self.shapes_ast = Some(schema);
        Ok(self.shapes_ast.as_ref().expect("just set"))
    }

    /// The parsed shapes AST, if any (form-IR projection input).
    pub fn shapes_ast(&self) -> Option<&ASTSchema> {
        self.shapes_ast.as_ref()
    }

    /// The raw shapes graph, if any (presentation/annotation reads).
    pub fn shapes_graph(&self) -> Option<&OxigraphInMemory> {
        self.shapes_graph.as_ref()
    }

    // ---- data ----------------------------------------------------------------

    /// Replace the live data graph with the parse of `text`.
    pub fn load_data(&mut self, text: &str, format: &RDFFormat) -> Result<(), FormError> {
        self.data = Self::parse_graph(text, format)?;
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

    /// Validate the whole live data graph against every loaded shape (native
    /// engine). The graph is cloned into a fresh validation store, leaving the
    /// session's live graph untouched.
    pub fn validate(&self) -> Result<ValidationOutcome, FormError> {
        let ir = self.compile()?;
        let mut gv = GraphValidation::new(Graph::from(self.data.clone()));
        let report = gv
            .validate(&ir, &ShaclValidationMode::Native)
            .map_err(|e| FormError::Validation(e.to_string()))?;
        Ok(ValidationOutcome {
            conforms: report.conforms(),
            results: report.results().clone(),
        })
    }

    /// Validate only the shape identified by `shape_id` (and its nested property
    /// shapes) against the data graph — shape-scoped: the shape's own targets are
    /// computed and validated, the rest of the schema is skipped.
    pub fn validate_shape(&self, shape_id: &str) -> Result<ValidationOutcome, FormError> {
        let ir = self.compile()?;
        let idx = resolve_idx(&ir, shape_id)?;
        let results = GraphValidation::new(Graph::from(self.data.clone()))
            .validate_scoped(&ir, idx, None)
            .map_err(|e| FormError::Validation(e.to_string()))?;
        Ok(outcome(results))
    }

    /// Validate a single `focus` node against the shape identified by `shape_id`
    /// (per-keystroke / per-field scope): no full-graph scan.
    pub fn validate_focus(&self, shape_id: &str, focus: &Object) -> Result<ValidationOutcome, FormError> {
        let ir = self.compile()?;
        let idx = resolve_idx(&ir, shape_id)?;
        let results = GraphValidation::new(Graph::from(self.data.clone()))
            .validate_scoped(&ir, idx, Some(focus))
            .map_err(|e| FormError::Validation(e.to_string()))?;
        Ok(outcome(results))
    }

    /// Compile the loaded shapes AST into the validator's internal representation.
    fn compile(&self) -> Result<IRSchema, FormError> {
        let ast = self.shapes_ast.as_ref().ok_or(FormError::NoShapes)?;
        IRSchema::try_from(ast).map_err(|e| FormError::Validation(e.to_string()))
    }
}

/// Resolve a shape's IRI string to its arena index in the compiled schema.
fn resolve_idx(ir: &IRSchema, shape_id: &str) -> Result<ShapeLabelIdx, FormError> {
    let shape_ref = Object::iri(IriS::new_unchecked(shape_id));
    ir.get_idx(&shape_ref)
        .copied()
        .ok_or_else(|| FormError::ShapeNotFound(shape_id.to_string()))
}

/// A scoped result list conforms exactly when it is empty (matching the report's
/// own `conforms()`).
fn outcome(results: Vec<ValidationResult>) -> ValidationOutcome {
    ValidationOutcome {
        conforms: results.is_empty(),
        results,
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
