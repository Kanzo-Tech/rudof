//! Findings: a validation report as the neutral model a UI lists — `Finding`
//! (one result) and `FindingGroup` (many results sharing a constraint and a
//! path) — with the RDF side, `RdfPlace`, kept here so the UI never reads RDF.
//!
//! A form wants one finding per result, filed under its node and path; a
//! corpus wants them grouped, counted and sampled. Both are made in Rust from
//! the façade's results, so a corpus with many results crosses the boundary as
//! its groups, never as every result.

use std::collections::{HashMap, HashSet};

use rudof_lib::form::{SHACLPath, ValidationOutcome, ValidationResult as RudofResult};
use serde::{Deserialize, Serialize};
use tsify::Tsify;

use crate::dto::{LangString, RudofUnchecked, TermValue, ValidationResult};
use crate::validate::{result_to_dto, unchecked_to_dto};

const SH: &str = "http://www.w3.org/ns/shacl#";

/// How many findings a group keeps as its sample when the caller does not say.
pub const SAMPLE: usize = 3;

/// How bad a finding is. `sh:Violation` and `sh:Warning` are themselves;
/// `sh:Info`, SHACL 1.2's `sh:Debug` and `sh:Trace`, and a shape's own
/// severity are `info`.
#[derive(Tsify, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Violation,
    Warning,
    Info,
}

impl Severity {
    pub fn of_iri(iri: &str) -> Self {
        match iri.strip_prefix(SH) {
            Some("Violation") => Severity::Violation,
            Some("Warning") => Severity::Warning,
            _ => Severity::Info,
        }
    }
}

/// The rule a finding breaks: a stable `id` and a `label` a reader reads.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rule {
    pub id: String,
    pub label: String,
}

/// One finding: one result, at one place.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Finding<Place> {
    pub severity: Severity,
    pub message: String,
    pub rule: Rule,
    pub place: Place,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

/// Many findings sharing a rule, a place's path and a severity: `count` of
/// them, at `places`, of which `sample` are a few in full.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FindingGroup<Place> {
    pub rule: Rule,
    pub severity: Severity,
    pub message: String,
    pub count: usize,
    pub places: Vec<Place>,
    pub sample: Vec<Finding<Place>>,
}

/// Where in RDF data a finding is: its focus node, the path it is about (the
/// canonical key the form's fields are indexed by, absent for a node-level
/// result) and the offending value, when there is one.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RdfPlace {
    pub focus: TermValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<TermValue>,
}

/// A finding in RDF data, with what the report says beyond the neutral model:
/// every language's message (a form re-words without validating again) and the
/// shape and constraint component the result came from.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RdfFinding {
    #[serde(flatten)]
    pub finding: Finding<RdfPlace>,
    pub messages: Vec<LangString>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_shape: Option<TermValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_constraint_component: Option<String>,
}

/// A group of findings in RDF data. Its `places` are its focus nodes, each
/// once, in the order the report gave them (a node failing the constraint on
/// two values is one place and two in `count`), with the group's path.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RdfFindingGroup {
    #[serde(flatten)]
    pub group: FindingGroup<RdfPlace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_shape: Option<TermValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_constraint_component: Option<String>,
}

/// A validation as one finding per result: what a form files under its fields.
// `hashmap_as_object`: a flattened struct serializes as a map, which must reach
// JavaScript as a plain object.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
#[tsify(hashmap_as_object)]
pub struct RdfFindings {
    pub conforms: bool,
    pub findings: Vec<RdfFinding>,
    pub unchecked: Vec<RudofUnchecked>,
}

/// A validation as groups of findings, the worst and largest first: what a
/// corpus lists.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug)]
#[tsify(hashmap_as_object)]
pub struct RdfFindingGroups {
    pub conforms: bool,
    pub groups: Vec<RdfFindingGroup>,
    pub unchecked: Vec<RudofUnchecked>,
}

/// How findings are worded and sampled.
#[derive(Tsify, Serialize, Deserialize, Clone, Default, Debug)]
pub struct FindingOptions {
    /// The reader's languages, preferred first (`navigator.languages`): a
    /// message is the first of them the result has (`en` answers `en-GB`),
    /// else the untagged one, else English, else any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    /// How many findings each group keeps in full; 3 when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<usize>,
}

/// One result, as the report DTO plus the readable form of its path.
pub struct Entry {
    pub result: ValidationResult,
    pub path_label: Option<String>,
}

impl Entry {
    pub fn of(r: &RudofResult) -> Self {
        Entry {
            result: result_to_dto(r),
            path_label: r.path().map(path_label),
        }
    }
}

/// The last segment of an IRI: what a reader calls the property or the type.
fn local(iri: &str) -> &str {
    let tail = &iri[iri.rfind(['#', '/']).map_or(0, |i| i + 1)..];
    if tail.is_empty() {
        iri
    } else {
        tail
    }
}

/// A path as `shapes::path_key` writes it, each IRI by its last segment.
pub fn path_label(path: &SHACLPath) -> String {
    let all = |paths: &[SHACLPath], sep: &str| paths.iter().map(path_label).collect::<Vec<_>>().join(sep);
    match path {
        SHACLPath::Predicate { pred } => local(pred.as_str()).to_string(),
        SHACLPath::Inverse { path } => format!("^{}", path_label(path)),
        SHACLPath::Sequence { paths } => format!("({})", all(paths, "/")),
        SHACLPath::Alternative { paths } => format!("({})", all(paths, "|")),
        SHACLPath::ZeroOrMore { path } => format!("{}*", path_label(path)),
        SHACLPath::OneOrMore { path } => format!("{}+", path_label(path)),
        SHACLPath::ZeroOrOne { path } => format!("{}?", path_label(path)),
    }
}

/// A shape's node as `Shapes.model()` names it: its IRI, or `_:` and a blank
/// node's label.
fn shape_id(node: &TermValue) -> String {
    match node.term_type.as_str() {
        "BlankNode" => format!("_:{}", node.value),
        _ => node.value.clone(),
    }
}

/// A constraint component by its readable name: `MinCount` for
/// `sh:MinCountConstraintComponent`.
fn constraint_name(component: &str) -> &str {
    let name = local(component);
    name.strip_suffix("ConstraintComponent")
        .filter(|n| !n.is_empty())
        .unwrap_or(name)
}

/// The rule a result breaks: its source shape and constraint component, which
/// identify the constraint; read as "path · Constraint".
fn rule_of(entry: &Entry) -> Rule {
    let r = &entry.result;
    let component = r.source_constraint_component.as_deref().unwrap_or("");
    let id = format!(
        "{}|{component}",
        r.source_shape.as_ref().map(shape_id).unwrap_or_default()
    );
    let label = [entry.path_label.as_deref().unwrap_or(""), constraint_name(component)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    Rule {
        label: if label.is_empty() { id.clone() } else { label },
        id,
    }
}

/// The primary subtag of a language tag, lowercased: `en` of `en-GB`.
fn primary(tag: &str) -> String {
    tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase()
}

/// The message for a reader of `languages`: the first language the result has,
/// exactly and then by primary subtag; else the untagged message, else English,
/// else the first.
pub fn word(messages: &[LangString], languages: &[String]) -> Option<String> {
    let exact = |tag: &str| messages.iter().find(|m| m.language.eq_ignore_ascii_case(tag));
    languages
        .iter()
        .find_map(|l| {
            exact(l).or_else(|| {
                let want = primary(l);
                messages
                    .iter()
                    .find(|m| !m.language.is_empty() && primary(&m.language) == want)
            })
        })
        .or_else(|| exact(""))
        .or_else(|| exact("en"))
        .or_else(|| messages.first())
        .map(|m| m.value.clone())
}

fn finding_of(entry: &Entry, rule: Rule, languages: &[String]) -> Finding<RdfPlace> {
    let r = &entry.result;
    Finding {
        severity: Severity::of_iri(&r.result_severity),
        message: word(&r.result_message, languages).unwrap_or_else(|| rule.label.clone()),
        rule,
        place: RdfPlace {
            focus: r.focus_node.clone(),
            path: r.path_key.clone(),
            value: r.value.clone(),
        },
        help: None,
    }
}

/// One finding per result, in the report's order.
pub fn findings(entries: impl IntoIterator<Item = Entry>, options: &FindingOptions) -> Vec<RdfFinding> {
    let languages = options.languages.as_deref().unwrap_or_default();
    entries
        .into_iter()
        .map(|entry| {
            let finding = finding_of(&entry, rule_of(&entry), languages);
            let r = entry.result;
            RdfFinding {
                finding,
                messages: r.result_message,
                source_shape: r.source_shape,
                source_constraint_component: r.source_constraint_component,
            }
        })
        .collect()
}

/// The results grouped by source shape, constraint component, path and
/// severity; the worst first, then the largest, then by rule and path. Each
/// result is folded in as it comes, so only the groups are kept.
pub fn groups(entries: impl IntoIterator<Item = Entry>, options: &FindingOptions) -> Vec<RdfFindingGroup> {
    let languages = options.languages.as_deref().unwrap_or_default();
    let sample = options.sample.unwrap_or(SAMPLE);
    let mut index: HashMap<(String, Option<String>, Severity), usize> = HashMap::new();
    let mut groups: Vec<(RdfFindingGroup, HashSet<(String, String)>)> = Vec::new();
    for entry in entries {
        let rule = rule_of(&entry);
        let severity = Severity::of_iri(&entry.result.result_severity);
        let key = (rule.id.clone(), entry.result.path_key.clone(), severity);
        let at = *index.entry(key).or_insert_with(|| {
            let first = finding_of(&entry, rule.clone(), languages);
            groups.push((
                RdfFindingGroup {
                    group: FindingGroup {
                        rule: rule.clone(),
                        severity,
                        message: first.message,
                        count: 0,
                        places: Vec::new(),
                        sample: Vec::new(),
                    },
                    source_shape: entry.result.source_shape.clone(),
                    source_constraint_component: entry.result.source_constraint_component.clone(),
                },
                HashSet::new(),
            ));
            groups.len() - 1
        });
        let (group, seen) = &mut groups[at];
        let group = &mut group.group;
        group.count += 1;
        if group.sample.len() < sample {
            group.sample.push(finding_of(&entry, rule, languages));
        }
        let focus = &entry.result.focus_node;
        if seen.insert((focus.term_type.clone(), focus.value.clone())) {
            group.places.push(RdfPlace {
                focus: focus.clone(),
                path: entry.result.path_key.clone(),
                value: None,
            });
        }
    }
    let mut groups: Vec<RdfFindingGroup> = groups.into_iter().map(|(g, _)| g).collect();
    groups.sort_by(|a, b| {
        let (a, b) = (&a.group, &b.group);
        a.severity
            .cmp(&b.severity)
            .then(b.count.cmp(&a.count))
            .then_with(|| a.rule.id.cmp(&b.rule.id))
            .then_with(|| a.places[0].path.cmp(&b.places[0].path))
    });
    groups
}

/// A façade outcome as one finding per result.
pub fn findings_of(outcome: &ValidationOutcome, options: &FindingOptions) -> RdfFindings {
    RdfFindings {
        conforms: outcome.conforms,
        findings: findings(outcome.results.iter().map(Entry::of), options),
        unchecked: unchecked_to_dto(&outcome.unchecked),
    }
}

/// A façade outcome as groups of findings.
pub fn groups_of(outcome: &ValidationOutcome, options: &FindingOptions) -> RdfFindingGroups {
    RdfFindingGroups {
        conforms: outcome.conforms,
        groups: groups(outcome.results.iter().map(Entry::of), options),
        unchecked: unchecked_to_dto(&outcome.unchecked),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudof_lib::form::{FormEngine, RDFFormat, Shapes};
    use wasm_bindgen_test::wasm_bindgen_test;

    const EX: &str = "http://example.org/";

    fn lang(language: &str, value: &str) -> LangString {
        LangString {
            value: value.into(),
            language: language.into(),
        }
    }

    fn entry(focus: &str, path: Option<&str>, value: Option<&str>, component: &str, severity: &str) -> Entry {
        Entry {
            result: ValidationResult {
                focus_node: TermValue::named(&format!("{EX}{focus}")),
                result_path: path.map(|p| TermValue::named(&format!("{EX}{p}"))),
                path_key: path.map(|p| format!("{EX}{p}")),
                value: value.map(|v| TermValue::literal(v, None, None)),
                result_message: vec![lang("en", &format!("{component} failed")), lang("es", "falla")],
                result_severity: format!("{SH}{severity}"),
                source_constraint_component: Some(format!("{SH}{component}ConstraintComponent")),
                source_shape: Some(TermValue::blank(&format!("b-{}", path.unwrap_or("node")))),
            },
            path_label: path.map(str::to_string),
        }
    }

    #[wasm_bindgen_test]
    fn severities_map_to_three() {
        assert_eq!(Severity::of_iri(&format!("{SH}Violation")), Severity::Violation);
        assert_eq!(Severity::of_iri(&format!("{SH}Warning")), Severity::Warning);
        assert_eq!(Severity::of_iri(&format!("{SH}Info")), Severity::Info);
        assert_eq!(Severity::of_iri(&format!("{SH}Debug")), Severity::Info);
        assert_eq!(Severity::of_iri(&format!("{EX}Mine")), Severity::Info);
    }

    #[wasm_bindgen_test]
    fn a_message_is_the_readers_language_then_untagged_then_english() {
        let m = [lang("en", "Missing"), lang("es", "Falta"), lang("", "plain")];
        let w = |l: &[&str]| word(&m, &l.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(w(&["es"]).as_deref(), Some("Falta"));
        assert_eq!(w(&["es-ES", "en"]).as_deref(), Some("Falta"));
        assert_eq!(w(&["fr", "EN-gb"]).as_deref(), Some("Missing"));
        assert_eq!(w(&["fr"]).as_deref(), Some("plain"));
        assert_eq!(word(&m[..2], &["fr".into()]).as_deref(), Some("Missing"));
        assert_eq!(word(&[], &[]), None);
    }

    #[wasm_bindgen_test]
    fn a_finding_is_one_result_with_its_rule_and_place() {
        let options = FindingOptions {
            languages: Some(vec!["es".into()]),
            sample: None,
        };
        let f = findings(
            [entry("n", Some("totalCost"), Some("-1"), "MinExclusive", "Violation")],
            &options,
        );
        assert_eq!(f.len(), 1);
        let finding = &f[0].finding;
        assert_eq!(finding.severity, Severity::Violation);
        assert_eq!(finding.message, "falla");
        assert_eq!(finding.rule.label, "totalCost · MinExclusive");
        assert_eq!(
            finding.rule.id,
            format!("_:b-totalCost|{SH}MinExclusiveConstraintComponent")
        );
        assert_eq!(finding.place.focus.value, format!("{EX}n"));
        assert_eq!(finding.place.path.as_deref(), Some(&*format!("{EX}totalCost")));
        assert_eq!(finding.place.value.as_ref().map(|v| v.value.as_str()), Some("-1"));
        assert_eq!(f[0].messages.len(), 2);
    }

    #[wasm_bindgen_test]
    fn results_group_by_constraint_path_and_severity() {
        let entries = vec![
            entry("a", Some("p"), Some("1"), "MinCount", "Warning"),
            entry("a", Some("q"), Some("1"), "Datatype", "Violation"),
            entry("b", Some("q"), Some("2"), "Datatype", "Violation"),
            entry("a", Some("q"), Some("3"), "Datatype", "Violation"),
            entry("c", Some("q"), Some("4"), "Datatype", "Violation"),
            entry("c", None, None, "Class", "Violation"),
        ];
        let options = FindingOptions {
            languages: None,
            sample: Some(2),
        };
        let g = groups(entries, &options);
        let summary: Vec<_> = g
            .iter()
            .map(|g| {
                (
                    g.group.rule.label.as_str(),
                    g.group.severity,
                    g.group.count,
                    g.group.places.len(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("q · Datatype", Severity::Violation, 4, 3),
                ("Class", Severity::Violation, 1, 1),
                ("p · MinCount", Severity::Warning, 1, 1),
            ]
        );
        let datatype = &g[0].group;
        assert_eq!(datatype.message, "Datatype failed");
        assert_eq!(
            datatype
                .places
                .iter()
                .map(|p| p.focus.value.as_str())
                .collect::<Vec<_>>(),
            [&*format!("{EX}a"), &*format!("{EX}b"), &*format!("{EX}c")]
        );
        assert!(datatype.places.iter().all(|p| p.value.is_none()));
        assert_eq!(datatype.sample.len(), 2);
        assert_eq!(datatype.sample[1].place.value.as_ref().unwrap().value, "2");
        assert_eq!(
            g[0].source_constraint_component.as_deref(),
            Some(&*format!("{SH}DatatypeConstraintComponent"))
        );
    }

    /// Through the façade: a property shape's results group under it, the
    /// label reads the path, and the message is in the asked-for language.
    #[wasm_bindgen_test]
    fn a_validation_groups_through_the_facade() {
        let prefixes = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix : <http://example.org/> .\n";
        let shapes = Shapes::parse(
            &format!(
                "{prefixes}:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :totalCost ; sh:minExclusive 0 ] ."
            ),
            &RDFFormat::Turtle,
            None,
        )
        .unwrap();
        let mut engine = FormEngine::new(shapes);
        engine
            .load_data(
                &format!("{prefixes}:a a :C ; :totalCost -1 , -2 . :b a :C ; :totalCost -3 . :c a :C ; :totalCost 5 ."),
                &RDFFormat::Turtle,
                None,
            )
            .unwrap();
        let outcome = engine.validate().unwrap();
        let options = FindingOptions {
            languages: Some(vec!["es".into()]),
            sample: None,
        };

        let grouped = groups_of(&outcome, &options);
        assert!(!grouped.conforms);
        assert_eq!(grouped.groups.len(), 1);
        let group = &grouped.groups[0].group;
        assert_eq!(group.rule.label, "totalCost · MinExclusive");
        assert_eq!((group.count, group.places.len(), group.sample.len()), (3, 2, 3));
        assert_eq!(group.severity, Severity::Violation);
        assert!(group.message.starts_with(char::is_alphabetic));

        let each = findings_of(&outcome, &options);
        assert_eq!(each.findings.len(), 3);
        assert!(each.findings.iter().all(|f| f.finding.rule == group.rule));
        let es = word(&each.findings[0].messages, &["es".into()]).unwrap();
        assert_eq!(each.findings[0].finding.message, es);
    }

    #[wasm_bindgen_test]
    fn the_typescript_extends_the_neutral_model() {
        assert!(Finding::<RdfPlace>::DECL.contains("export interface Finding<Place>"));
        assert!(FindingGroup::<RdfPlace>::DECL.contains("sample: Finding<Place>[]"));
        assert!(
            RdfFinding::DECL.contains("extends Finding<RdfPlace>"),
            "{}",
            RdfFinding::DECL
        );
        assert!(RdfFindingGroup::DECL.contains("extends FindingGroup<RdfPlace>"));
        assert!(
            Severity::DECL.contains(r#""violation" | "warning" | "info""#),
            "{}",
            Severity::DECL
        );
    }

    /// Across the boundary a flattened finding is a plain object, as its
    /// TypeScript says, not a `Map`.
    #[wasm_bindgen_test]
    fn a_finding_reaches_javascript_as_a_plain_object() {
        let report = RdfFindings {
            conforms: false,
            findings: findings(
                [entry("n", Some("p"), None, "MinCount", "Violation")],
                &Default::default(),
            ),
            unchecked: vec![],
        };
        let js: wasm_bindgen::JsValue = crate::to_js(&report).unwrap().into();
        let get = |v: &wasm_bindgen::JsValue, k: &str| js_sys::Reflect::get(v, &k.into()).unwrap();
        let first = js_sys::Array::from(&get(&js, "findings")).get(0);
        assert!(!wasm_bindgen::JsCast::is_instance_of::<js_sys::Map>(&first));
        assert_eq!(
            get(&get(&first, "rule"), "label").as_string().as_deref(),
            Some("p · MinCount")
        );
        assert_eq!(get(&first, "severity").as_string().as_deref(), Some("violation"));
    }
}
