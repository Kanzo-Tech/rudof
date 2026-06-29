//! Marshalling between the façade's validation outcome and the `RudofReport`
//! ABI DTO. The validation itself (full / shape-scoped / single-focus) runs in
//! `rudof_lib::form::FormEngine`; here we only map rudof's `ValidationResult`
//! into the vocabulary-agnostic report the JS side consumes.

use rudof_lib::form::{IriS, Object, SHACLPath, Severity, ValidationOutcome, ValidationResult};

use crate::dto::{RudofReport, RudofResult, TermValue};
use crate::object_to_value;

/// Map a façade [`ValidationOutcome`] into the ABI report DTO.
pub fn report_from_outcome(outcome: &ValidationOutcome) -> RudofReport {
    RudofReport {
        conforms: outcome.conforms,
        results: outcome.results.iter().map(result_to_dto).collect(),
    }
}

/// Convert a focus `TermValue` into the rudof `Object` the focus-scoped validator
/// entry point expects (normally a `NamedNode`/`BlankNode` resource).
pub fn focus_object(focus: &TermValue) -> Result<Object, String> {
    Object::try_from(crate::term_to_object(focus)).map_err(|e| e.to_string())
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
