//! Marshalling between the façade's validation outcome and the `ValidationReport`
//! ABI DTO. The validation itself (full / shape-scoped / single-focus) runs in
//! `rudof_lib::form::FormEngine`; here we only map rudof's `ValidationResult`
//! into the vocabulary-agnostic report the JS side consumes.

use rudof_lib::form::{IriS, Object, SHACLPath, Severity, Unchecked, ValidationOutcome};

use crate::dto::{LangString, RudofUnchecked, TermValue, ValidationReport, ValidationResult};
use crate::object_to_value;
use crate::shapes::path_key;

/// Map a façade [`ValidationOutcome`] into the ABI report DTO.
pub fn report_from_outcome(outcome: &ValidationOutcome) -> ValidationReport {
    ValidationReport {
        conforms: outcome.conforms,
        results: outcome.results.iter().map(result_to_dto).collect(),
        unchecked: unchecked_to_dto(&outcome.unchecked),
    }
}

/// The shapes a validation or a fragment left out, and why.
pub fn unchecked_to_dto(unchecked: &[Unchecked]) -> Vec<RudofUnchecked> {
    unchecked
        .iter()
        .map(|u| RudofUnchecked {
            shape: object_to_term(&u.shape),
            reason: u.reason.to_string(),
        })
        .collect()
}

/// Convert a focus `TermValue` into the rudof `Object` the focus-scoped validator
/// entry point expects (normally a `NamedNode`/`BlankNode` resource).
pub fn focus_object(focus: &TermValue) -> Result<Object, String> {
    Object::try_from(crate::term_to_object(focus)).map_err(|e| e.to_string())
}

pub(crate) fn result_to_dto(r: &rudof_lib::form::ValidationResult) -> ValidationResult {
    ValidationResult {
        focus_node: object_to_term(r.focus_node()),
        result_path: r.path().and_then(path_to_term),
        // Never re-derived here. `shapes::path_key` is the one that produced the
        // keys the fields are indexed by, so calling anything else — however
        // identical it looked — would file errors under keys no field has, and
        // would do it silently.
        path_key: r.path().map(path_key),
        value: r.value().map(object_to_term),
        // Preserve the language key of each message: `Some(lang)` → its tag,
        // `None` (engine default / untagged) → "". The JS side selects by locale.
        result_message: r
            .message()
            .messages()
            .iter()
            .map(|(lang, value)| LangString {
                value: value.clone(),
                language: lang.as_ref().map(|l| l.as_str().to_string()).unwrap_or_default(),
            })
            .collect(),
        result_severity: severity_iri(r.severity()),
        source_constraint_component: object_iri(r.constraint_component()),
        source_shape: r.source().map(object_to_term),
    }
}

/// rudof's `Object` term → ABI `TermValue`, via the existing oxrdf converter
/// (`Term: From<Object>` is guaranteed by the `Rdf` trait).
pub(crate) fn object_to_term(o: &Object) -> TermValue {
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
pub(crate) fn path_to_term(p: &SHACLPath) -> Option<TermValue> {
    match p {
        SHACLPath::Predicate { pred } => Some(TermValue::named(pred.as_str())),
        _ => None,
    }
}

pub(crate) fn severity_iri(s: &Severity) -> String {
    let iri: IriS = s.into();
    iri.as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudof_lib::form::{FormEngine, RDFFormat, Shapes};
    use wasm_bindgen_test::wasm_bindgen_test;

    const PREFIXES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix : <http://example.org/> .\n";

    /// One shape requiring `:p` twice. `message` is the shape's own
    /// `sh:message`, when it has one.
    fn shapes(message: &str) -> Shapes {
        let shapes = format!(
            "{PREFIXES}:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :p ; sh:minCount 2 {message} ] ."
        );
        Shapes::parse(&shapes, &RDFFormat::Turtle, None).unwrap()
    }

    /// Those shapes over one node with no `:p`.
    fn engine(shapes: Shapes) -> FormEngine {
        let mut engine = FormEngine::new(shapes);
        engine
            .load_data(&format!("{PREFIXES}:n a :C ."), &RDFFormat::Turtle, None)
            .unwrap();
        engine
    }

    fn messages(engine: &FormEngine) -> Vec<(String, String)> {
        let report = report_from_outcome(&engine.validate().unwrap());
        assert_eq!(report.results.len(), 1);
        let mut m: Vec<_> = report.results[0]
            .result_message
            .iter()
            .map(|l| (l.language.clone(), l.value.clone()))
            .collect();
        m.sort();
        m
    }

    fn pair(lang: &str, text: &str) -> (String, String) {
        (lang.to_string(), text.to_string())
    }

    #[wasm_bindgen_test]
    fn the_shapes_own_messages_are_the_only_ones() {
        let m = messages(&engine(shapes("; sh:message \"Falta\"@es , \"Missing\"@en")));
        assert_eq!(m, [pair("en", "Missing"), pair("es", "Falta")]);
    }

    #[wasm_bindgen_test]
    fn a_result_names_the_blank_property_shape_the_model_lists() {
        let engine = engine(shapes(""));
        let model = crate::shapes::schema_to_json(engine.shapes().ast(), engine.shapes().graph());
        let source = report_from_outcome(&engine.validate().unwrap()).results[0]
            .source_shape
            .clone()
            .unwrap();
        assert_eq!(source.term_type, "BlankNode");
        assert_eq!(model.node_shapes[0].properties[0].id, format!("_:{}", source.value));
    }

    #[wasm_bindgen_test]
    fn a_silent_shape_gets_one_tagged_message_per_catalog_language() {
        let m = messages(&engine(shapes("")));
        assert_eq!(
            m,
            [
                pair("ca", "Calen com a mínim 2 valor(s)"),
                pair("en", "At least 2 value(s) required"),
                pair("es", "Se requieren al menos 2 valor(es)"),
            ]
        );
    }

    #[wasm_bindgen_test]
    fn loaded_messages_add_a_language_and_reword_one() {
        let mut shapes = shapes("");
        shapes
            .load_messages(
                &format!(
                    "{PREFIXES}sh:MinCountConstraintComponent sh:message \"Au moins {{$minCount}}\"@fr , \"Need {{$minCount}}\"@en ."
                ),
                &RDFFormat::Turtle,
            )
            .unwrap();
        let m = messages(&engine(shapes.clone()));
        assert_eq!(m.len(), 4);
        assert!(m.contains(&pair("fr", "Au moins 2")));
        assert!(m.contains(&pair("en", "Need 2")));
        assert!(shapes.load_messages("nonsense", &RDFFormat::Turtle).is_err());
        assert_eq!(messages(&engine(shapes)).len(), 4);
    }
}
