//! SHACL validation over the live data graph, using the (forked) shacl
//! validator that runs without the `sparql` feature and without rayon on wasm.
//! Maps rudof's `ValidationReport` into the `RudofReport` ABI DTO.

use rudof_rdf::rdf_core::SHACLPath;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_impl::OxigraphInMemory;
use rudof_iri::IriS;
use shacl::ast::ASTSchema;
use shacl::ir::{IRSchema, ShapeLabelIdx};
use shacl::types::Severity;
use shacl::validator::ShaclValidationMode;
use shacl::validator::processor::{GraphValidation, ShaclProcessor};
use shacl::validator::report::ValidationResult;
use shacl::validator::store::Graph;

use crate::dto::{RudofReport, RudofResult, TermValue};
use crate::{object_to_value, term_to_object};

/// Validate `data` against the parsed shapes, in the native engine. The data
/// graph is cloned into a fresh validation store so the session's live graph is
/// left untouched.
pub fn validate(data: &OxigraphInMemory, ast: &ASTSchema) -> Result<RudofReport, String> {
    let ir = IRSchema::try_from(ast).map_err(|e| e.to_string())?;
    let mut gv = GraphValidation::new(Graph::from(data.clone()));
    let report = gv
        .validate(&ir, &ShaclValidationMode::Native)
        .map_err(|e| e.to_string())?;

    let results = report.results().iter().map(result_to_dto).collect();
    Ok(RudofReport { conforms: report.conforms(), results })
}

/// Validate only the shape identified by `shape_id` (and its nested property
/// shapes) against the data graph — shape-scoped validation. Honors the
/// `validate(shapeId)` ABI: the shape's own targets are computed and validated,
/// the rest of the schema is skipped.
pub fn validate_shape(data: &OxigraphInMemory, ast: &ASTSchema, shape_id: &str) -> Result<RudofReport, String> {
    let ir = IRSchema::try_from(ast).map_err(|e| e.to_string())?;
    let idx = resolve_idx(&ir, shape_id)?;
    let gv = GraphValidation::new(Graph::from(data.clone()));
    let results = gv.validate_scoped(&ir, idx, None).map_err(|e| e.to_string())?;
    Ok(report_of(results))
}

/// Validate a single `focus` node against the shape identified by `shape_id`,
/// via the validator's scoped `validate_focus` entry point. This is the
/// per-keystroke / per-field revalidation path: no full-graph scan, the focus
/// term is the only allocation beyond the engine's class index.
pub fn validate_focus(
    data: &OxigraphInMemory,
    ast: &ASTSchema,
    shape_id: &str,
    focus: &TermValue,
) -> Result<RudofReport, String> {
    let ir = IRSchema::try_from(ast).map_err(|e| e.to_string())?;
    let idx = resolve_idx(&ir, shape_id)?;
    let focus = Object::try_from(term_to_object(focus)).map_err(|e| e.to_string())?;
    let gv = GraphValidation::new(Graph::from(data.clone()));
    let results = gv.validate_scoped(&ir, idx, Some(&focus)).map_err(|e| e.to_string())?;
    Ok(report_of(results))
}

/// Resolve a shape's IRI string to its arena index in the compiled schema.
fn resolve_idx(ir: &IRSchema, shape_id: &str) -> Result<ShapeLabelIdx, String> {
    let shape_ref = Object::iri(IriS::new_unchecked(shape_id));
    ir.get_idx(&shape_ref)
        .copied()
        .ok_or_else(|| format!("shape not found in shapes graph: {shape_id}"))
}

/// Build the ABI report from a flat result list (scoped paths have no
/// `ValidationReport`; conformance is "no results", matching the report's own
/// `conforms()`).
fn report_of(results: Vec<ValidationResult>) -> RudofReport {
    let conforms = results.is_empty();
    let results = results.iter().map(result_to_dto).collect();
    RudofReport { conforms, results }
}

fn result_to_dto(r: &ValidationResult) -> RudofResult {
    RudofResult {
        focus_node: object_to_term(r.focus_node()),
        path: r.path().and_then(path_to_term),
        value: r.value().map(object_to_term),
        message: r.message().messages().values().cloned().collect(),
        severity: Some(severity_iri(r.severity())),
        source_constraint_component: object_iri(r.constraint_component()),
    }
}

/// rudof's `Object` term → ABI `TermValue`, via the existing oxrdf converter
/// (`Term: From<Object>` is guaranteed by the `Rdf` trait).
fn object_to_term(o: &Object) -> TermValue {
    object_to_value(&o.clone().into())
}

fn object_iri(o: &Object) -> Option<String> {
    match o {
        Object::Iri(i) => Some(i.as_str().to_string()),
        _ => None,
    }
}

/// Only a plain predicate path maps to a single result-path term; complex paths
/// have no single-term representation in the report.
fn path_to_term(p: &SHACLPath) -> Option<TermValue> {
    match p {
        SHACLPath::Predicate { pred } => Some(TermValue::named(pred.as_str())),
        _ => None,
    }
}

fn severity_iri(s: &Severity) -> String {
    let iri: IriS = s.into();
    iri.as_str().to_string()
}
