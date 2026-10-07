//! Which editor a property gets, by the scoring system of SHACL 1.2 UI (Editor's
//! Draft, "Scoring System"; the pinned data is in `spec/shacl12-ui`, see its
//! README).
//!
//! The specification defines a *score function*: every widget has `shui:WidgetScore`
//! entries, each a pair of SHACL shapes and a number, and the widgets whose shapes
//! a node satisfies are ranked by that number. The shapes and the numbers are data
//! published with the specification, and they are what this module runs; there is
//! no rule in this crate that says which editor a datatype gets. The matcher shapes
//! are evaluated by the engine's own validator ([`NodeChecker`]).
//!
//! The form IR is about shapes, not about a data node, so the score function runs
//! here in its "no focus node" form: the *shape node* is the property shape, and
//! only the `shui:shapesGraphShape` of a matcher is evaluated. A matcher that has
//! only a `shui:dataGraphShape` — "the value is an `xsd:date`" — needs a value to
//! look at and does not match.

use std::collections::{BTreeSet, HashMap, HashSet};

use rudof_lib::form::{
    ASTSchema, BlankNode, BuildRDF, FormEngine, FormError, Literal, NamedNode, NamedOrBlankNode, NodeChecker, Object,
    OxigraphInMemory, RDFFormat, Term,
};

use crate::dto::EditorScore;
use crate::index::SubjectIndex;

const SH: &str = "http://www.w3.org/ns/shacl#";
const SHUI: &str = "http://www.w3.org/ns/shacl-ui/";
const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

/// The default `shui:defaultWidgetScore`, the score of a widget a shape declares
/// with `shui:editor` when the scoring graph does not score it (the "40" band of
/// the Score Conventions). The global configuration that could override it is not
/// read.
const DEFAULT_WIDGET_SCORE: i64 = 40;

/// The scoring graph: the vocabulary, the matcher shapes and the 26 widget files of
/// the Editor's Draft, verbatim, concatenated into one Turtle document (the files
/// declare their own prefixes, and blank nodes are scoped to the document, so
/// nothing collides).
const SCORING_GRAPH: &str = concat!(
    include_str!("../spec/shacl12-ui/shacl-ui.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/score-shapes.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/auto-complete-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/blank-node-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/boolean-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/date-picker-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/date-time-picker-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/details-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/enum-select-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/instances-select-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/iri-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/number-field-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/rich-text-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/sub-class-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/text-area-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/text-area-with-lang-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/text-field-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/editors/text-field-with-lang-editor.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/blank-node-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/details-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/html-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/hyperlink-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/image-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/iri-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/label-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/lang-string-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/literal-viewer.ttl"),
    "\n",
    include_str!("../spec/shacl12-ui/widgets/viewers/value-table-viewer.ttl")
);

fn shui(local: &str) -> String {
    format!("{SHUI}{local}")
}

/// A `shui:WidgetMatcher`: the shapes a shape node must satisfy.
struct Matcher {
    /// `shui:shapesGraphShape`, all of which must hold. The specification's
    /// Matcher function reads "the value" in the singular, but the scoring data
    /// gives several (`shui:prefersAutoCompleteEditor, shui:hasClassConstraint`)
    /// and only their conjunction reproduces the scores the prose states.
    shapes: Vec<Object>,
    /// Whether the matcher has a `shui:dataGraphShape`.
    has_data_shape: bool,
}

/// A `shui:WidgetScore`.
struct Score {
    widget: String,
    score: f64,
    matcher: Matcher,
}

/// The prepared scoring graph, ready to score the property shapes of one shapes
/// graph.
pub(crate) struct Scoring {
    checker: NodeChecker,
    /// The `shui:WidgetScore` entries in the order the score function visits them:
    /// by descending score, then by the code points of the widget IRI.
    scores: Vec<Score>,
    accept: HashMap<String, Vec<Matcher>>,
    editors: HashSet<String>,
}

impl Scoring {
    /// Prepare the scoring graph for `shapes` (the shapes graph, of which `schema`
    /// is the parse) and index it.
    ///
    /// The vendored scoring data is well-formed (a test parses it), so a failure
    /// here is a build defect and not something to hand to the caller.
    pub(crate) fn new(schema: &ASTSchema, shapes: &OxigraphInMemory) -> Self {
        Self::try_new(schema, shapes).expect("the vendored SHACL UI scoring graph is well-formed")
    }

    fn try_new(schema: &ASTSchema, shapes: &OxigraphInMemory) -> Result<Self, FormError> {
        let mut scoring = FormEngine::parse_graph(SCORING_GRAPH, &RDFFormat::Turtle, None)?;
        let declared_editors = prepare(&mut scoring, shapes, schema)?;
        let index = SubjectIndex::new(&scoring);

        let mut scores = index
            .instances(&shui("WidgetScore"))
            .map(|s| {
                Ok(Score {
                    widget: index.widget(s)?,
                    score: index.number(s, &shui("score"))?,
                    matcher: index.matcher(s),
                })
            })
            .collect::<Result<Vec<_>, FormError>>()?;
        // Score descending, then widget IRI ascending by code point (a `str`
        // compares by UTF-8 bytes, which orders code points the same way).
        scores.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.widget.cmp(&b.widget)));

        let mut accept: HashMap<String, Vec<Matcher>> = HashMap::new();
        for m in index.instances(&shui("WidgetAcceptMatcher")) {
            accept.entry(index.widget(m)?).or_default().push(index.matcher(m));
        }

        let mut editors = index.widgets_of_class(&shui("Editor"));
        editors.extend(declared_editors);

        Ok(Self {
            checker: NodeChecker::new(&scoring, shapes)?,
            scores,
            accept,
            editors,
        })
    }

    /// The score function for the shape node `shape`, keeping the editors: every
    /// editor with a matching, accepted `shui:WidgetScore`, best first.
    ///
    /// The scoring graph mixes editors and viewers and the function has no
    /// parameter to choose between them, so the result is filtered by widget class
    /// (here first, which only saves evaluating viewer matchers). A widget with
    /// several matching scores is reported once, at its highest — the
    /// normalisation the specification's Widget Selection asks for.
    pub(crate) fn editors(&self, shape: &Object) -> Vec<EditorScore> {
        let mut results: Vec<EditorScore> = Vec::new();
        let mut accepted: HashMap<&str, bool> = HashMap::new();
        for s in &self.scores {
            if !self.editors.contains(&s.widget) || results.iter().any(|r| r.editor == s.widget) {
                continue;
            }
            if !self.matches(&s.matcher, shape) {
                continue;
            }
            let accept = *accepted.entry(&s.widget).or_insert_with(|| {
                self.accept
                    .get(&s.widget)
                    .is_none_or(|ms| ms.iter().all(|m| self.matches(m, shape)))
            });
            if accept {
                results.push(EditorScore {
                    editor: s.widget.clone(),
                    score: s.score,
                });
            }
        }
        results
    }

    /// The matcher function with no focus node.
    fn matches(&self, matcher: &Matcher, shape: &Object) -> bool {
        // "If no focus node is given, a value for the shui:dataGraphShape property
        // is present, and no value for the shui:shapesGraphShape is present,
        // return false."
        if matcher.has_data_shape && matcher.shapes.is_empty() {
            return false;
        }
        // The shape node is the focus node, the shapes graph the data graph, the
        // scoring graph the shapes graph. With no focus node that is all.
        //
        // A shape the scoring graph cannot evaluate is malformed, and the
        // specification's answer to a malformed shape is `false` (it also asks for
        // a logged warning; this crate has no channel to log to).
        matcher
            .shapes
            .iter()
            .all(|s| self.checker.conforms(s, shape).unwrap_or(false))
    }
}

/// Scoring graph preparation: add a `shui:WidgetScore` for each widget the shapes
/// declare with `shui:editor` or `shui:viewer` that the scoring graph does not
/// score, so that a declared widget is never ignored. Returns the widgets declared
/// by `shui:editor`, which are editors whatever the scoring graph says of them.
fn prepare(
    scoring: &mut OxigraphInMemory,
    shapes: &OxigraphInMemory,
    schema: &ASTSchema,
) -> Result<BTreeSet<String>, FormError> {
    let index = SubjectIndex::new(scoring);
    let scored: HashSet<String> = index
        .instances(&shui("WidgetScore"))
        .map(|s| index.widget(s))
        .collect::<Result<_, _>>()?;

    let mut declared_editors = BTreeSet::new();
    let mut additions = Vec::new();
    for property in ["editor", "viewer"] {
        let property = shui(property);
        // The IRI values of `property` at nodes that are shapes; anything else
        // does not declare a widget.
        let widgets: BTreeSet<String> = shapes
            .quads()
            .filter(|q| q.predicate.as_str() == property)
            .filter(|q| subject_object(&q.subject).is_some_and(|s| schema.get_shape(&s).is_some()))
            .filter_map(|q| match q.object {
                Term::NamedNode(w) => Some(w.into_string()),
                _ => None,
            })
            .collect();
        for widget in widgets {
            if property.ends_with("editor") {
                declared_editors.insert(widget.clone());
            }
            if !scored.contains(&widget) {
                additions.push((property.clone(), widget));
            }
        }
    }
    drop(index);

    for (property, widget) in additions {
        add_declared_widget_score(scoring, &property, &widget)?;
    }
    Ok(declared_editors)
}

/// `[] a shui:WidgetScore ; shui:widget W ; shui:score D ; shui:shapesGraphShape [
/// a sh:NodeShape ; sh:property [ sh:path P ; sh:hasValue W ] ]`
fn add_declared_widget_score(graph: &mut OxigraphInMemory, property: &str, widget: &str) -> Result<(), FormError> {
    let node = |b: &BlankNode| NamedOrBlankNode::BlankNode(b.clone());
    let iri = |i: &str| Term::NamedNode(NamedNode::new_unchecked(i));
    let (score, shape, property_shape) = (BlankNode::default(), BlankNode::default(), BlankNode::default());
    let triples = [
        (node(&score), RDF_TYPE.to_string(), iri(&shui("WidgetScore"))),
        (node(&score), shui("widget"), iri(widget)),
        (
            node(&score),
            shui("score"),
            Term::Literal(Literal::new_typed_literal(
                DEFAULT_WIDGET_SCORE.to_string(),
                NamedNode::new_unchecked(XSD_INTEGER),
            )),
        ),
        (node(&score), shui("shapesGraphShape"), Term::BlankNode(shape.clone())),
        (node(&shape), RDF_TYPE.to_string(), iri(&format!("{SH}NodeShape"))),
        (
            node(&shape),
            format!("{SH}property"),
            Term::BlankNode(property_shape.clone()),
        ),
        (node(&property_shape), format!("{SH}path"), iri(property)),
        (node(&property_shape), format!("{SH}hasValue"), iri(widget)),
    ];
    for (s, p, o) in triples {
        graph
            .add_triple(s, NamedNode::new_unchecked(p), o)
            .map_err(|e| FormError::Graph(e.to_string()))?;
    }
    Ok(())
}

fn subject_object(s: &NamedOrBlankNode) -> Option<Object> {
    match s {
        NamedOrBlankNode::NamedNode(n) => Object::try_from(Term::NamedNode(n.clone())).ok(),
        NamedOrBlankNode::BlankNode(b) => Object::try_from(Term::BlankNode(b.clone())).ok(),
    }
}

impl SubjectIndex {
    fn instances<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a NamedOrBlankNode> + 'a {
        let class = NamedNode::new_unchecked(class);
        self.by_subject
            .keys()
            .filter(move |s| self.values(s, RDF_TYPE).any(|t| *t == Term::NamedNode(class.clone())))
    }

    fn widget(&self, matcher: &NamedOrBlankNode) -> Result<String, FormError> {
        match self.values(matcher, &shui("widget")).collect::<Vec<_>>().as_slice() {
            [Term::NamedNode(w)] => Ok(w.as_str().to_string()),
            _ => Err(FormError::Parse(format!("{matcher} has no single shui:widget IRI"))),
        }
    }

    fn number(&self, node: &NamedOrBlankNode, predicate: &str) -> Result<f64, FormError> {
        match self.values(node, predicate).collect::<Vec<_>>().as_slice() {
            [Term::Literal(l)] => l
                .value()
                .parse()
                .map_err(|_| FormError::Parse(format!("{node}: {predicate} is not a number"))),
            _ => Err(FormError::Parse(format!("{node} has no single {predicate}"))),
        }
    }

    fn matcher(&self, node: &NamedOrBlankNode) -> Matcher {
        Matcher {
            shapes: self
                .values(node, &shui("shapesGraphShape"))
                .filter_map(|t| Object::try_from(t.clone()).ok())
                .collect(),
            has_data_shape: self.values(node, &shui("dataGraphShape")).next().is_some(),
        }
    }

    /// The IRIs typed with `class` or with a subclass of it.
    fn widgets_of_class(&self, class: &str) -> HashSet<String> {
        let mut classes: HashSet<String> = HashSet::from([class.to_string()]);
        loop {
            let before = classes.len();
            for (sub, edges) in &self.by_subject {
                if let NamedOrBlankNode::NamedNode(sub) = sub {
                    let is_sub = edges.iter().any(|(p, o)| {
                        p == RDFS_SUBCLASS_OF && matches!(o, Term::NamedNode(sup) if classes.contains(sup.as_str()))
                    });
                    if is_sub {
                        classes.insert(sub.as_str().to_string());
                    }
                }
            }
            if classes.len() == before {
                break;
            }
        }
        self.by_subject
            .iter()
            .filter_map(|(s, edges)| match s {
                NamedOrBlankNode::NamedNode(n)
                    if edges.iter().any(|(p, o)| {
                        p == RDF_TYPE && matches!(o, Term::NamedNode(t) if classes.contains(t.as_str()))
                    }) =>
                {
                    Some(n.as_str().to_string())
                },
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{EditorSource, PropertyShapeIR, ShapeModelJson};
    use rudof_lib::form::{RDFFormat, Shapes};
    use wasm_bindgen_test::wasm_bindgen_test;

    const PREFIXES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix shui: <http://www.w3.org/ns/shacl-ui/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix : <http://example.org/> .
:Other a sh:NodeShape .
"#;

    fn project(shapes: &str) -> ShapeModelJson {
        let shapes = Shapes::parse(&format!("{PREFIXES}{shapes}"), &RDFFormat::Turtle, None).expect("shapes parse");
        crate::shapes::schema_to_json(shapes.ast(), shapes.graph())
    }

    /// The property shapes `:P0`, `:P1`, … of one node shape, one for each body:
    /// the shape's own triples, after its `sh:path`.
    fn properties(bodies: &[&str]) -> Vec<PropertyShapeIR> {
        let names = (0..bodies.len())
            .map(|i| format!(":P{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let shapes: String = bodies
            .iter()
            .enumerate()
            .map(|(i, body)| format!(":P{i} sh:path :p{i} ; {body} .\n"))
            .collect();
        let model = project(&format!(
            ":S a sh:NodeShape ; sh:targetClass :C ; sh:property {names} .\n{shapes}"
        ));
        let mut ps = model
            .node_shapes
            .into_iter()
            .find(|n| n.id.ends_with("/S"))
            .expect("the node shape")
            .properties;
        ps.sort_by_key(|p| {
            let id = p.id.as_str();
            id.rsplit('P')
                .next()
                .and_then(|n| n.parse::<usize>().ok())
                .expect("P<n>")
        });
        ps
    }

    /// An editor as the tests spell it: a built-in one by its local name.
    fn short(iri: &str) -> String {
        iri.strip_prefix(SHUI).unwrap_or(iri).to_string()
    }

    fn ranked(p: &PropertyShapeIR) -> Vec<(String, f64)> {
        p.presentation
            .editors
            .iter()
            .map(|e| (short(&e.editor), e.score))
            .collect()
    }

    /// The editors the score function returns for one property shape.
    fn candidates(body: &str) -> Vec<(String, f64)> {
        ranked(&properties(&[body])[0])
    }

    fn pair(editor: &str, score: f64) -> (String, f64) {
        (editor.to_string(), score)
    }

    /// The scoring data itself: the numbers the module runs are the ones in the
    /// specification's files — 71 `shui:WidgetScore` and 4 `shui:WidgetAcceptMatcher`
    /// — and 16 of the widgets are editors.
    #[wasm_bindgen_test]
    fn the_vendored_scoring_graph_is_the_editors_draft_data() {
        let scoring = Scoring::try_new(&ASTSchema::default(), &OxigraphInMemory::new()).expect("well-formed");
        assert_eq!(scoring.scores.len(), 71);
        assert_eq!(scoring.accept.values().map(Vec::len).sum::<usize>(), 4);
        assert_eq!(scoring.editors.len(), 16);
        // Descending by score, then ascending by widget IRI.
        assert!(scoring
            .scores
            .windows(2)
            .all(|w| w[0].score > w[1].score || (w[0].score == w[1].score && w[0].widget <= w[1].widget)));
    }

    /// One row per editor: the shape fact that selects it, and the score of the
    /// `shui:WidgetScore` that matched. The specification's rows that need a value
    /// to look at ("the value is a blank node") cannot fire on a shape alone; the
    /// editors that have only such rows are chosen by being declared with
    /// `shui:editor`, which scores 40.
    #[wasm_bindgen_test]
    fn each_editor_is_selected_by_its_shape_fact() {
        let rows: [(&str, &str, f64); 16] = [
            ("AutoCompleteEditor", "sh:nodeKind sh:IRI ; sh:class :C", 10.0),
            ("BlankNodeEditor", "shui:editor shui:BlankNodeEditor", 40.0),
            ("BooleanEditor", "sh:datatype xsd:boolean", 10.0),
            ("DatePickerEditor", "sh:datatype xsd:date", 10.0),
            ("DateTimePickerEditor", "sh:datatype xsd:dateTime", 10.0),
            ("DetailsEditor", "shui:editor shui:DetailsEditor", 40.0),
            ("EnumSelectEditor", "sh:in ( 1 2 )", 30.0),
            ("InstancesSelectEditor", "sh:class :C", 0.0),
            ("IRIEditor", "sh:nodeKind sh:IRI", 20.0),
            ("NumberFieldEditor", "sh:datatype xsd:integer", 10.0),
            ("RichTextEditor", "sh:datatype rdf:HTML", 10.0),
            (
                "SubClassEditor",
                "shui:editor shui:SubClassEditor ; sh:rootClass :C",
                40.0,
            ),
            ("TextAreaEditor", "sh:singleLine false", 30.0),
            (
                "TextAreaWithLangEditor",
                "sh:datatype rdf:langString ; sh:singleLine false",
                30.0,
            ),
            ("TextFieldEditor", "sh:datatype xsd:string", 10.0),
            ("TextFieldWithLangEditor", "sh:datatype rdf:langString", 10.0),
        ];
        let bodies: Vec<&str> = rows.iter().map(|r| r.1).collect();
        for ((editor, body, score), p) in rows.iter().zip(properties(&bodies)) {
            let found = ranked(&p);
            assert!(
                found.contains(&pair(editor, *score)),
                "{body}: expected {editor} at {score}, got {found:?}"
            );
        }
    }

    /// `sh:datatype xsd:string` is the plain case, and its whole result is worth
    /// seeing: four editors, ranked by the scores the specification gives them.
    #[wasm_bindgen_test]
    fn a_string_has_four_candidates_ranked_by_score() {
        assert_eq!(
            candidates("sh:datatype xsd:string"),
            vec![
                pair("TextFieldEditor", 10.0),
                pair("TextAreaEditor", 5.0),
                pair("TextFieldWithLangEditor", 1.0),
                pair("TextAreaWithLangEditor", 0.0),
            ]
        );
    }

    /// `presentation.editor` is the first of `presentation.editors`.
    #[wasm_bindgen_test]
    fn the_editor_is_the_first_candidate() {
        let p = &properties(&["sh:datatype xsd:date"])[0];
        assert_eq!(
            p.presentation.editor.as_deref(),
            Some("http://www.w3.org/ns/shacl-ui/DatePickerEditor")
        );
        assert_eq!(
            p.presentation.editors.first().map(|e| e.editor.clone()),
            p.presentation.editor
        );
    }

    /// An editor the shape declares scores 40 (the default `shui:defaultWidgetScore`)
    /// and takes the property over a shape fact that scores less.
    #[wasm_bindgen_test]
    fn a_declared_editor_scores_forty_and_wins() {
        let found = candidates("sh:datatype xsd:string ; shui:editor shui:TextAreaEditor");
        assert_eq!(found.first(), Some(&pair("TextAreaEditor", 40.0)));
        assert!(found.contains(&pair("TextFieldEditor", 10.0)));
    }

    /// A widget the scoring graph does not know is given a `shui:WidgetScore` by
    /// scoring graph preparation, at 40, and it is an editor because it was
    /// declared with `shui:editor`.
    #[wasm_bindgen_test]
    fn a_declared_editor_the_scoring_graph_does_not_know_is_prepared_at_forty() {
        assert_eq!(
            candidates("sh:datatype xsd:date ; shui:editor :MyEditor"),
            vec![
                pair("http://example.org/MyEditor", 40.0),
                pair("DatePickerEditor", 10.0),
            ]
        );
    }

    /// Equal scores are ordered by the code points of the widget IRIs, not by the
    /// order the shape lists them in.
    #[wasm_bindgen_test]
    fn equal_scores_are_ordered_by_widget_iri() {
        for body in ["shui:editor :Zed, :Alpha", "shui:editor :Alpha, :Zed"] {
            assert_eq!(
                candidates(body),
                vec![
                    pair("http://example.org/Alpha", 40.0),
                    pair("http://example.org/Zed", 40.0)
                ],
                "{body}"
            );
        }
    }

    /// `shui:viewer` is prepared like `shui:editor`, and the result keeps editors
    /// only: a declared viewer is not offered as an editor.
    #[wasm_bindgen_test]
    fn a_declared_viewer_is_not_an_editor() {
        assert_eq!(
            candidates("sh:datatype xsd:date ; shui:viewer shui:LabelViewer"),
            vec![pair("DatePickerEditor", 10.0)]
        );
    }

    /// `sh:singleLine` reaches the matchers. The specification's accept matchers
    /// leave a text area out when it is `true` and a text field out when it is
    /// `false`, so the same string property is a text field or a text area by it.
    #[wasm_bindgen_test]
    fn sh_single_line_selects_between_field_and_area() {
        let ps = properties(&[
            "sh:datatype xsd:string ; sh:singleLine true",
            "sh:datatype xsd:string ; sh:singleLine false",
        ]);
        assert_eq!(
            ranked(&ps[0]),
            vec![pair("TextFieldEditor", 10.0), pair("TextFieldWithLangEditor", 1.0),],
            "singleLine true: the text areas are not accepted"
        );
        assert_eq!(
            ranked(&ps[1]),
            vec![pair("TextAreaEditor", 30.0), pair("TextAreaWithLangEditor", 30.0),],
            "singleLine false: the text fields are not accepted"
        );
        assert_eq!(ps[0].presentation.single_line, Some(true));
        assert_eq!(ps[1].presentation.single_line, Some(false));
    }

    /// The `#` namespace of the First Public Working Draft is not the Editor's
    /// Draft's, and is not recognised: the declaration is ignored and the property
    /// gets its scored default.
    #[wasm_bindgen_test]
    fn an_editor_in_the_old_namespace_is_not_recognised() {
        let found = candidates(
            "sh:datatype xsd:date ; <http://www.w3.org/ns/shacl-ui#editor> <http://www.w3.org/ns/shacl-ui#TextAreaEditor>",
        );
        assert_eq!(found, vec![pair("DatePickerEditor", 10.0)]);
    }

    /// A shape that states its type only through `sh:or` takes the editor of its
    /// first branch. This is not the specification's, which scores the shape node
    /// alone and returns nothing here: the source says `branch`, and `editors`, which
    /// holds only what the score function returned, is empty.
    #[wasm_bindgen_test]
    fn a_disjunction_takes_the_editor_of_its_first_branch() {
        let p = &properties(&["sh:or ( [ sh:datatype xsd:date ] [ sh:datatype xsd:dateTime ] )"])[0];
        assert_eq!(
            p.presentation.editor.as_deref(),
            Some("http://www.w3.org/ns/shacl-ui/DatePickerEditor")
        );
        assert_eq!(p.presentation.editor_source, Some(EditorSource::Branch));
        assert!(p.presentation.editors.is_empty());
        let branches = p.logical.or.as_ref().expect("the branches are projected");
        assert_eq!(
            branches[1].presentation.editor.as_deref(),
            Some("http://www.w3.org/ns/shacl-ui/DateTimePickerEditor")
        );
        assert_eq!(branches[1].presentation.editor_source, Some(EditorSource::Scored));
    }

    /// Nothing scores and there is no branch: our own rule, a nested form for
    /// `sh:node` and a text field otherwise, and it says so.
    #[wasm_bindgen_test]
    fn nothing_scored_falls_back_to_a_text_field_or_a_nested_form() {
        let ps = properties(&["sh:minCount 1", "sh:node :Other"]);
        for (p, editor) in ps.iter().zip(["TextFieldEditor", "DetailsEditor"]) {
            assert_eq!(p.presentation.editor.as_deref().map(short).as_deref(), Some(editor));
            assert_eq!(p.presentation.editor_source, Some(EditorSource::Fallback));
            assert!(p.presentation.editors.is_empty());
        }
    }

    /// An editor the shape declares has source `declared`; one the score function
    /// picks has `scored`. A declared editor that does not win is not the source.
    #[wasm_bindgen_test]
    fn the_source_tells_declared_from_scored() {
        let ps = properties(&[
            "sh:datatype xsd:string ; shui:editor shui:TextAreaEditor",
            "sh:datatype xsd:string",
            "sh:datatype xsd:string ; sh:singleLine true ; shui:editor shui:TextAreaEditor",
        ]);
        let sources: Vec<_> = ps.iter().map(|p| p.presentation.editor_source).collect();
        assert_eq!(
            sources,
            vec![
                Some(EditorSource::Declared),
                Some(EditorSource::Scored),
                Some(EditorSource::Scored)
            ]
        );
        // The third declares a text area that `sh:singleLine true` does not accept.
        assert_eq!(
            ps[2].presentation.editor.as_deref().map(short).as_deref(),
            Some("TextFieldEditor")
        );
    }

    /// The shapes graph's `rdfs:label`s of the path's predicate, with their language
    /// tags (SHACL UI, Property Labels, step 3): only for a predicate path, only
    /// that predicate's, and absent when there are none.
    #[wasm_bindgen_test]
    fn the_shapes_graphs_labels_of_the_predicate_are_projected() {
        let model = project(
            r#"@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
:title rdfs:label "Title"@en, "Título"@es ; rdfs:comment "not a label" .
:S a sh:NodeShape ; sh:targetClass :C ;
  sh:property [ sh:path :title ], [ sh:path :other ], [ sh:path [ sh:inversePath :title ] ] ."#,
        );
        let ps = &model
            .node_shapes
            .iter()
            .find(|n| n.id.ends_with("/S"))
            .unwrap()
            .properties;
        // `^…/title` ends with "/title" too, and the properties come in no fixed
        // order: only a predicate path (no `^` prefix) is meant.
        let labels = |key: &str| -> Vec<(String, String)> {
            ps.iter()
                .find(|p| !p.path_key.starts_with('^') && p.path_key.ends_with(key))
                .unwrap()
                .presentation
                .path_labels
                .iter()
                .map(|l| (l.language.clone(), l.value.clone()))
                .collect()
        };
        assert_eq!(
            labels("/title"),
            vec![
                ("en".to_string(), "Title".to_string()),
                ("es".to_string(), "Título".to_string())
            ]
        );
        assert!(labels("/other").is_empty());
        assert!(ps
            .iter()
            .find(|p| p.path_key.starts_with('^'))
            .unwrap()
            .presentation
            .path_labels
            .is_empty());
    }

    /// The data graph's `rdfs:label`s of the predicate come with the projection of a
    /// focus node (SHACL UI, Property Labels, step 2).
    #[wasm_bindgen_test]
    fn the_data_graphs_labels_of_the_predicate_are_projected() {
        let mut engine = FormEngine::new(
            Shapes::parse(
                &format!(
                    "{PREFIXES}:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :title ], [ sh:path :other ] ."
                ),
                &RDFFormat::Turtle,
                None,
            )
            .expect("shapes parse"),
        );
        engine
            .load_data(
                r#"@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . @prefix : <http://example.org/> .
:title rdfs:label "Titre"@fr . :d a :C ; :title "x" ."#,
                &RDFFormat::Turtle,
                None,
            )
            .expect("data parse");
        let form = crate::project::project_form(
            &engine,
            engine.shapes().ast(),
            &crate::dto::TermValue::named("http://example.org/d"),
            "http://example.org/S",
        );
        let title = form.properties.iter().find(|p| p.path_key.ends_with("/title")).unwrap();
        assert_eq!(title.path_labels.len(), 1);
        assert_eq!(
            (
                title.path_labels[0].language.as_str(),
                title.path_labels[0].value.as_str()
            ),
            ("fr", "Titre")
        );
        let other = form.properties.iter().find(|p| p.path_key.ends_with("/other")).unwrap();
        assert!(other.path_labels.is_empty());
    }

    /// Every case where the editor of a property changed when the twelve
    /// hand-written first-match rules gave way to the score function and our own
    /// fallback. `old` is what those rules returned, recorded by running them on the
    /// same shapes (the commit before the score function, d14a85216). Rows with
    /// `old == new` are the ones that did not change.
    #[wasm_bindgen_test]
    fn editor_selection_old_rules_against_the_score_function() {
        use EditorSource::{Branch, Declared, Fallback, Scored};
        // (shape body, old editor, new editor, score of the score function, source)
        #[rustfmt::skip]
        let rows: &[(&str, &str, &str, Option<f64>, EditorSource)] = &[
            ("sh:datatype xsd:string", "TextFieldEditor", "TextFieldEditor", Some(10.0), Scored),
            ("sh:datatype xsd:boolean", "BooleanEditor", "BooleanEditor", Some(10.0), Scored),
            ("sh:datatype xsd:date", "DatePickerEditor", "DatePickerEditor", Some(10.0), Scored),
            ("sh:datatype xsd:dateTime", "DateTimePickerEditor", "DateTimePickerEditor", Some(10.0), Scored),
            ("sh:datatype xsd:integer", "NumberFieldEditor", "NumberFieldEditor", Some(10.0), Scored),
            ("sh:datatype xsd:int", "NumberFieldEditor", "NumberFieldEditor", Some(10.0), Scored),
            ("sh:datatype xsd:positiveInteger", "NumberFieldEditor", "TextFieldEditor", None, Fallback),
            ("sh:datatype xsd:nonPositiveInteger", "NumberFieldEditor", "TextFieldEditor", None, Fallback),
            ("sh:datatype rdf:langString", "TextFieldWithLangEditor", "TextFieldWithLangEditor", Some(10.0), Scored),
            ("sh:datatype rdf:HTML", "RichTextEditor", "RichTextEditor", Some(10.0), Scored),
            ("sh:datatype xsd:anyURI", "IRIEditor", "TextFieldEditor", None, Fallback),
            ("sh:datatype xsd:gYear", "TextFieldEditor", "TextFieldEditor", None, Fallback),
            ("sh:datatype :MyType", "TextFieldEditor", "TextFieldEditor", Some(0.0), Scored),
            ("sh:in ( 1 2 )", "EnumSelectEditor", "EnumSelectEditor", Some(30.0), Scored),
            ("sh:class :C", "AutoCompleteEditor", "InstancesSelectEditor", Some(0.0), Scored),
            ("sh:class :C ; sh:nodeKind sh:IRI", "AutoCompleteEditor", "AutoCompleteEditor", Some(10.0), Scored),
            ("sh:nodeKind sh:IRI", "IRIEditor", "IRIEditor", Some(20.0), Scored),
            ("sh:nodeKind sh:Literal", "TextFieldEditor", "TextFieldEditor", Some(0.0), Scored),
            ("sh:nodeKind sh:BlankNode", "TextFieldEditor", "TextFieldEditor", None, Fallback),
            ("sh:node :Other", "DetailsEditor", "DetailsEditor", None, Fallback),
            ("sh:datatype xsd:string ; sh:singleLine false", "TextFieldEditor", "TextAreaEditor", Some(30.0), Scored),
            ("sh:or ( [ sh:datatype xsd:date ] [ sh:datatype xsd:dateTime ] )", "DatePickerEditor", "DatePickerEditor", None, Branch),
            ("sh:datatype xsd:string ; shui:editor shui:TextAreaEditor", "TextAreaEditor", "TextAreaEditor", Some(40.0), Declared),
            ("sh:minCount 1", "TextFieldEditor", "TextFieldEditor", None, Fallback),
        ];
        let bodies: Vec<&str> = rows.iter().map(|r| r.0).collect();
        let mut changed = Vec::new();
        for ((body, old, new, score, source), p) in rows.iter().zip(properties(&bodies)) {
            let got = p.presentation.editor.as_deref().map(short);
            assert_eq!(got.as_deref(), Some(*new), "{body}");
            assert_eq!(p.presentation.editor_source, Some(*source), "{body}");
            assert_eq!(p.presentation.editors.first().map(|e| e.score), *score, "{body}");
            if old != new {
                changed.push(format!("{body}: {old} -> {new}"));
            }
        }
        assert_eq!(
            changed,
            vec![
                "sh:datatype xsd:positiveInteger: NumberFieldEditor -> TextFieldEditor",
                "sh:datatype xsd:nonPositiveInteger: NumberFieldEditor -> TextFieldEditor",
                "sh:datatype xsd:anyURI: IRIEditor -> TextFieldEditor",
                "sh:class :C: AutoCompleteEditor -> InstancesSelectEditor",
                "sh:datatype xsd:string ; sh:singleLine false: TextFieldEditor -> TextAreaEditor",
            ]
        );
    }
}
