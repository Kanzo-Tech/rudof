//! Maps rudof's SHACL AST (`shacl::ast`) into the vocabulary-agnostic
//! `ShapeModelJson` — node/property shapes with typed constraint cores, an open
//! component bag, and the standard SHACL/DASH presentation annotations
//! (`sh:name`/`order`/`group`, `shui:editor`/`viewer`) read from the shapes
//! graph. Emitting JSON here keeps the JS side from re-parsing SHACL.

use rudof_lib::form::{
    ASTComponent, ASTNodeShape, ASTPropertyShape, ASTSchema, ASTShape, BlankNode, ConcreteLiteral, IriRef, NamedNode,
    NamedOrBlankNode, NodeKind, Object, OxigraphInMemory, SHACLPath, Target, Term as OxTerm, Value,
};
use std::collections::HashMap;

use rudof_lib::form::shui::editors;

use crate::dto::*;
use crate::index::SubjectIndex;
use crate::object_to_value;
use crate::scoring::Scoring;

const SH: &str = "http://www.w3.org/ns/shacl#";
const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
const SHUI: &str = "http://www.w3.org/ns/shacl-ui/";
const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// rudof's SHACL parser is validation-focused and does not populate the
/// presentation/annotation terms (sh:name/description/order/group, shui:editor)
/// or sh:PropertyGroup metadata. We read those directly from the shapes graph.
pub fn schema_to_json(schema: &ASTSchema, graph: &OxigraphInMemory) -> ShapeModelJson {
    // Prepared and compiled once, then asked about every property shape.
    let scoring = Scoring::new(schema, graph);
    let index = SubjectIndex::new(graph);
    let mut node_shapes = Vec::new();
    let mut by_target_class = Vec::new();

    for (id, shape) in schema.iter() {
        if let ASTShape::NodeShape(ns) = shape {
            let ir = node_shape_to_ir(id, ns, schema, &index, &scoring);
            for tc in &ir.target_classes {
                by_target_class.push((tc.clone(), ir.id.clone()));
            }
            node_shapes.push(ir);
        }
    }

    ShapeModelJson {
        node_shapes,
        groups: read_groups(graph),
        by_target_class,
    }
}

fn node_shape_to_ir(
    id: &Object,
    ns: &ASTNodeShape,
    schema: &ASTSchema,
    graph: &SubjectIndex,
    scoring: &Scoring,
) -> NodeShapeIR {
    let target_classes: Vec<String> = ns
        .targets()
        .iter()
        .filter_map(|t| match t {
            Target::Class(o) | Target::ImplicitClass(o) => object_iri(o),
            _ => None,
        })
        .collect();

    let properties = ns
        .property_shapes()
        .iter()
        .filter_map(|pref| match schema.get_shape(pref) {
            Some(ASTShape::PropertyShape(ps)) => Some(property_to_ir(ps, schema, graph, scoring)),
            _ => None,
        })
        .collect();

    let conditionals = conditionals_of(ns, schema)
        .into_iter()
        .map(|c| ConditionalIR {
            condition_id: object_str(c.cond),
            then_id: c.then.map(object_str),
            then: resolve_then_else(c.then, schema, graph, scoring),
            els: resolve_then_else(c.els, schema, graph, scoring),
        })
        .collect();

    let (closed, ignored_properties) = closed_info(ns.components());

    // A node shape may be a combination of shapes as readily as a property shape
    // may. Reading them here is what lets a consumer see through a `sh:node` that
    // points at a shape whose whole content is an `sh:or`.
    //
    // An `sh:or` that states a conditional is left out: it is already reported in
    // `conditionals`, and a consumer reading it again as a disjunction would offer
    // "not the condition" as a kind of value to choose.
    let plain: Vec<ASTComponent> = ns
        .components()
        .iter()
        .filter(|c| !matches!(c, ASTComponent::Or(refs) if implication(refs, schema).is_some()))
        .cloned()
        .collect();
    let logical = shape_core(&plain, schema, graph, scoring, 0).logical;

    NodeShapeIR {
        id: object_str(id),
        instance_class: target_classes.first().cloned(),
        target_classes,
        properties,
        conditionals,
        logical,
        closed,
        ignored_properties,
        deactivated: flag(ns.is_deactivated()),
    }
}

/// `true` -> `Some(true)`; `false` -> `None`, so an active shape adds nothing to
/// the payload (the DTO skips `None`).
fn flag(on: bool) -> Option<bool> {
    on.then_some(true)
}

/// `sh:closed` and its `sh:ignoredProperties` escape hatch, read off the parsed
/// `Closed` component — the two are one component in SHACL (§4.8.1) and are
/// projected together, because `closed: true` alone would tell a consumer to
/// reject exactly the properties the profile listed as permitted.
///
/// The parser stores the exemptions in a `HashSet`, whose iteration order varies
/// run to run; sort so the emitted payload is stable.
fn closed_info(components: &[ASTComponent]) -> (Option<bool>, Vec<String>) {
    for c in components {
        if let ASTComponent::Closed {
            is_closed,
            ignored_properties,
            ..
        } = c
        {
            let mut ignored: Vec<String> = ignored_properties.iter().map(|i| i.as_str().to_string()).collect();
            ignored.sort();
            return (flag(*is_closed), ignored);
        }
    }
    (None, Vec::new())
}

/// A conditional requirement declared on a node shape: when the focus node
/// conforms to `cond`, the property shapes of `then` apply; otherwise those of
/// `els`.
pub(crate) struct Conditional<'a> {
    pub cond: &'a Object,
    pub then: Option<&'a Object>,
    pub els: Option<&'a Object>,
}

/// Every conditional a node shape declares — the one place that decides what
/// counts as one, so the shapes projection and the value projection cannot
/// disagree about it.
///
/// Three spellings are read. SHACL Core has no conditional component; what it has
/// is `sh:or` and `sh:not` (§4.6.3, §4.6.1), and "if C then T" is written with them
/// as the material implication `sh:or ( [ sh:not C ] T )`. That is the SHACL 1.0
/// form. SHACL 1.2 Core adds `sh:targetWhere` (§3.1.3.6), the second standard one:
/// a shape T with `sh:targetWhere C` requires whatever conforms to C to conform to
/// T. `sh:if` / `sh:then` / `sh:else` is this engine's own component. All three
/// arrive here as the same record.
///
/// A where target is global, it does not hang off a shape: the conditionals it
/// contributes belong to every node shape of the schema, because the form of any
/// focus node is subject to it, and it simply never holds for a focus that cannot
/// conform to C. They are not attached to T, which is the consequent itself, nor
/// to C, and one already stated by the shape's own implication is not repeated.
pub(crate) fn conditionals_of<'a>(ns: &'a ASTNodeShape, schema: &'a ASTSchema) -> Vec<Conditional<'a>> {
    let mut out: Vec<Conditional<'a>> = ns
        .components()
        .iter()
        .filter_map(|c| match c {
            ASTComponent::If { cond, then_, else_ } => Some(Conditional {
                cond,
                then: then_.as_ref(),
                els: else_.as_ref(),
            }),
            ASTComponent::Or(refs) => implication(refs, schema),
            _ => None,
        })
        .collect();
    for (id, shape) in schema.iter() {
        let ASTShape::NodeShape(target) = shape else { continue };
        if id == ns.id() || branch_property_shapes(Some(id), schema).is_empty() {
            continue;
        }
        for cond in target.targets().iter().filter_map(|t| match t {
            Target::Where(cond) => Some(cond),
            _ => None,
        }) {
            let repeated = out.iter().any(|c| c.cond == cond && c.then == Some(id));
            if cond != ns.id() && !repeated {
                out.push(Conditional {
                    cond,
                    then: Some(id),
                    els: None,
                });
            }
        }
    }
    out
}

/// Read `sh:or ( [ sh:not C ] T )`, in either order, as "if C then T".
///
/// Deliberately narrow, because every other `sh:or` is a disjunction and must stay
/// one: exactly two branches, exactly one of them a negation and nothing else, and
/// the other one a shape that contributes property shapes — the fields the
/// condition brings in. A disjunction of value kinds has no such branch.
fn implication<'a>(refs: &'a [Object], schema: &'a ASTSchema) -> Option<Conditional<'a>> {
    let [a, b] = refs else { return None };
    let (cond, then) = match (negated(a, schema), negated(b, schema)) {
        (Some(cond), None) => (cond, b),
        (None, Some(cond)) => (cond, a),
        _ => return None,
    };
    if branch_property_shapes(Some(then), schema).is_empty() {
        return None;
    }
    Some(Conditional {
        cond,
        then: Some(then),
        els: None,
    })
}

/// The shape `o` negates, when negating it is all `o` does.
fn negated<'a>(o: &Object, schema: &'a ASTSchema) -> Option<&'a Object> {
    match schema.get_shape(o)? {
        ASTShape::NodeShape(ns) if ns.property_shapes().is_empty() => match ns.components().as_slice() {
            [ASTComponent::Not(c)] => Some(c),
            _ => None,
        },
        _ => None,
    }
}

/// The property shapes a conditional branch contributes: a node shape contributes
/// each of its `property_shapes()`; a property shape contributes itself; anything
/// unresolved or absent contributes none.
pub(crate) fn branch_property_shapes<'a>(obj: Option<&Object>, schema: &'a ASTSchema) -> Vec<&'a ASTPropertyShape> {
    let Some(obj) = obj else { return Vec::new() };
    match schema.get_shape(obj) {
        Some(ASTShape::NodeShape(ns)) => ns
            .property_shapes()
            .iter()
            .filter_map(|pref| match schema.get_shape(pref) {
                Some(ASTShape::PropertyShape(ps)) => Some(ps.as_ref()),
                _ => None,
            })
            .collect(),
        Some(ASTShape::PropertyShape(ps)) => vec![ps.as_ref()],
        _ => Vec::new(),
    }
}

fn resolve_then_else(
    obj: Option<&Object>,
    schema: &ASTSchema,
    graph: &SubjectIndex,
    scoring: &Scoring,
) -> Vec<PropertyShapeIR> {
    branch_property_shapes(obj, schema)
        .into_iter()
        .map(|ps| property_to_ir(ps, schema, graph, scoring))
        .collect()
}

/// The constraints a shape carries, read off its component list — the one thing a
/// node shape and a property shape have in common, and everything a disjunction
/// branch consists of.
#[derive(Default)]
struct ShapeCore {
    value: ValueConstraints,
    logical: LogicalConstraints,
    cardinality: Cardinality,
    node: Option<String>,
}

/// How deep a chain of shapes referring to shapes is followed.
///
/// Anonymous `sh:or` lists cannot be cyclic, but named shapes can refer to each
/// other, and since branches may now be node shapes the walk can reach one. A cap
/// rather than a visited set because the depth a real profile uses is 1: DCAT-AP's
/// helper shapes are one hop, and nothing in the corpus goes past two.
const MAX_SHAPE_DEPTH: u8 = 8;

fn shape_core(
    components: &[ASTComponent],
    schema: &ASTSchema,
    graph: &SubjectIndex,
    scoring: &Scoring,
    depth: u8,
) -> ShapeCore {
    let mut value = ValueConstraints::default();
    let mut logical = LogicalConstraints::default();
    let mut cardinality = Cardinality::default();
    let mut node = None;

    for c in components {
        match c {
            // `value.datatype` is one IRI. A list of datatypes (SHACL 1.2) is "one of
            // these", which the payload has no field for yet, so it is not projected.
            ASTComponent::Datatype(iris) => {
                if let [iri] = iris.as_slice() {
                    value.datatype = Some(iriref_str(iri));
                }
            },
            // As for `sh:datatype`, a list of classes or node kinds (SHACL 1.2)
            // has no field in the payload yet.
            ASTComponent::Class(classes) => {
                if let [class] = classes.as_slice() {
                    value.class_iri = Some(iriref_str(class));
                }
            },
            ASTComponent::NodeKind(kinds) => {
                if let [kind] = kinds.as_slice() {
                    value.node_kind = Some(nodekind_iri(kind));
                }
            },
            ASTComponent::MinCount(n) => cardinality.min = Some(*n as i64),
            ASTComponent::MaxCount(n) => cardinality.max = Some(*n as i64),
            ASTComponent::MinLength(n) => value.min_length = Some(*n as i64),
            ASTComponent::MaxLength(n) => value.max_length = Some(*n as i64),
            ASTComponent::MinInclusive(l) => value.min_inclusive = concrete_f64(l),
            ASTComponent::MaxInclusive(l) => value.max_inclusive = concrete_f64(l),
            ASTComponent::MinExclusive(l) => value.min_exclusive = concrete_f64(l),
            ASTComponent::MaxExclusive(l) => value.max_exclusive = concrete_f64(l),
            ASTComponent::Pattern { pattern, flags } => {
                value.pattern = Some(pattern.clone());
                value.flags = flags.clone();
            },
            ASTComponent::UniqueLang(b) => value.unique_lang = Some(*b),
            ASTComponent::LanguageIn(langs) => {
                value.language_in = Some(langs.iter().map(|l| l.as_str().to_string()).collect())
            },
            ASTComponent::In(vals) => value.in_values = Some(vals.iter().map(value_to_term).collect()),
            ASTComponent::HasValue(v) => value.has_value = Some(value_to_term(v)),
            ASTComponent::Node(o) => node = Some(object_str(o)),
            ASTComponent::Or(refs) => logical.or = Some(resolve_branches(refs, schema, graph, scoring, depth)),
            ASTComponent::Xone(refs) => logical.xone = Some(resolve_branches(refs, schema, graph, scoring, depth)),
            ASTComponent::And(refs) => logical.and = Some(resolve_branches(refs, schema, graph, scoring, depth)),
            ASTComponent::Not(o) => {
                logical.not = branch_to_ir(o, schema, graph, scoring, depth).map(Box::new);
            },
            _ => {},
        }
    }

    ShapeCore {
        value,
        logical,
        cardinality,
        node,
    }
}

fn property_to_ir(
    ps: &ASTPropertyShape,
    schema: &ASTSchema,
    graph: &SubjectIndex,
    scoring: &Scoring,
) -> PropertyShapeIR {
    property_to_ir_at(ps, schema, graph, scoring, 0)
}

fn property_to_ir_at(
    ps: &ASTPropertyShape,
    schema: &ASTSchema,
    graph: &SubjectIndex,
    scoring: &Scoring,
    depth: u8,
) -> PropertyShapeIR {
    let ShapeCore {
        mut value,
        logical,
        cardinality,
        node,
    } = shape_core(ps.components(), schema, graph, scoring, depth);

    // sh:defaultValue is an annotation, not a validation constraint, so rudof's
    // parser doesn't surface it — read it from the shapes graph like the others.
    value.default_value = default_value(graph, ps.id());

    let mut presentation = presentation(graph, scoring, ps.id(), ps.components());
    if let SHACLPath::Predicate { pred } = ps.path() {
        presentation.path_labels = graph.labels().of(pred.as_str());
    }

    PropertyShapeIR {
        id: object_str(ps.id()),
        path: path_to_ir(ps.path()),
        path_key: path_key(ps.path()),
        cardinality,
        value,
        logical,
        node,
        presentation,
        components: read_components(graph, ps.id()),
        deactivated: flag(ps.is_deactivated()),
    }
}

/// Read sh:defaultValue for a property-shape node from the shapes graph.
fn default_value(graph: &SubjectIndex, node: &Object) -> Option<TermValue> {
    let subj = object_to_subject(node)?;
    let pred = format!("{SH}defaultValue");
    let value = graph.values(&subj, &pred).next().map(object_to_value);
    value
}

/// Every (predicate, object) on the property-shape node, grouped by predicate IRI
/// — the open extension point so rules/widgets can read terms the typed core does
/// not model (custom vocab, new SHACL 1.2 components).
fn read_components(graph: &SubjectIndex, node: &Object) -> Vec<ComponentIR> {
    let Some(subj) = object_to_subject(node) else {
        return Vec::new();
    };
    let mut by_pred: HashMap<String, Vec<TermValue>> = HashMap::new();
    for (pred, object) in graph.about(&subj) {
        by_pred.entry(pred.clone()).or_default().push(object_to_value(object));
    }
    by_pred
        .into_iter()
        .map(|(iri, values)| {
            let mut params = HashMap::new();
            params.insert("value".to_string(), values);
            ComponentIR { iri, params }
        })
        .collect()
}

/// The members of an `sh:and` / `sh:or` / `sh:xone` list.
///
/// Both kinds of shape are kept. A member with no `sh:path` parses as a NODE
/// shape — that is what SHACL says it is — and keeping only property shapes
/// therefore discarded the entire common case: every `sh:or ( [sh:datatype …] … )`
/// and `sh:or ( [sh:class …] … )` in a published profile arrived as an empty list,
/// which is how a construct that is parsed, mapped and emitted still reached
/// consumers saying nothing.
fn resolve_branches(
    refs: &[Object],
    schema: &ASTSchema,
    graph: &SubjectIndex,
    scoring: &Scoring,
    depth: u8,
) -> Vec<ShapeIR> {
    refs.iter()
        .filter_map(|o| branch_to_ir(o, schema, graph, scoring, depth))
        .collect()
}

fn branch_to_ir(o: &Object, schema: &ASTSchema, graph: &SubjectIndex, scoring: &Scoring, depth: u8) -> Option<ShapeIR> {
    if depth >= MAX_SHAPE_DEPTH {
        return None;
    }
    let next = depth + 1;
    match schema.get_shape(o) {
        Some(ASTShape::PropertyShape(ps)) => Some(property_to_ir_at(ps, schema, graph, scoring, next).into()),
        Some(ASTShape::NodeShape(ns)) => {
            let ShapeCore {
                value,
                logical,
                cardinality,
                node,
            } = shape_core(ns.components(), schema, graph, scoring, next);
            let presentation = presentation(graph, scoring, o, ns.components());
            Some(ShapeIR {
                id: object_str(o),
                path: None,
                path_key: None,
                cardinality,
                value,
                logical,
                node,
                presentation,
                components: read_components(graph, o),
                deactivated: flag(ns.is_deactivated()),
            })
        },
        _ => None,
    }
}

// ---- annotations read from the shapes graph ---------------------------------

fn object_to_subject(o: &Object) -> Option<NamedOrBlankNode> {
    match o {
        Object::Iri(i) => Some(NamedOrBlankNode::NamedNode(NamedNode::new_unchecked(i.as_str()))),
        Object::BlankNode(b) => Some(NamedOrBlankNode::BlankNode(BlankNode::new_unchecked(b))),
        _ => None,
    }
}

/// Read sh:name / sh:description / sh:order / sh:group / sh:singleLine /
/// shui:viewer for a shape node from the shapes graph (rudof's AST omits these),
/// and choose its editor.
///
/// The editor is the best result of the SHACL UI score function for the shape node
/// (see [`crate::scoring`]); `shui:editor` is not read here, since a declared
/// editor takes part in the scoring like every other candidate. When the score
/// function returns nothing, the two rules of our own apply, in order: the editor
/// of the first `sh:or` / `sh:xone` branch, then [`fallback_editor`]. Neither is
/// SHACL UI, and `editor_source` says which one produced the editor.
fn presentation(
    graph: &SubjectIndex,
    scoring: &Scoring,
    node: &Object,
    components: &[ASTComponent],
) -> PresentationHints {
    let mut p = PresentationHints::default();
    let Some(subj) = object_to_subject(node) else { return p };

    let mut declared = Vec::new();
    for (pred, object) in graph.about(&subj) {
        if pred == &format!("{SH}name") {
            if let Some(ls) = lang_string(object) {
                p.names.push(ls);
            }
        } else if pred == &format!("{SH}description") {
            if let Some(ls) = lang_string(object) {
                p.descriptions.push(ls);
            }
        } else if pred == &format!("{SH}order") {
            p.order = literal_value(object).and_then(|v| v.parse().ok());
        } else if pred == &format!("{SH}group") {
            p.group_id = iri_value(object);
        } else if pred == &format!("{SH}singleLine") {
            p.single_line = literal_value(object).and_then(|v| v.parse().ok());
        } else if pred == &format!("{SHUI}viewer") {
            p.viewer = iri_value(object);
        } else if pred == &format!("{SHUI}editor") {
            declared.extend(iri_value(object));
        }
    }

    p.editors = scoring.editors(node);
    let (editor, source) = match p.editors.first() {
        Some(best) if declared.contains(&best.editor) => (best.editor.clone(), EditorSource::Declared),
        Some(best) => (best.editor.clone(), EditorSource::Scored),
        None => match first_branch(components).and_then(|b| scoring.editors(b).into_iter().next()) {
            Some(best) => (best.editor, EditorSource::Branch),
            None => (fallback_editor(components).to_string(), EditorSource::Fallback),
        },
    };
    p.editor = Some(editor);
    p.editor_source = Some(source);
    p
}

/// The first `sh:or` branch, else the first `sh:xone` branch: a shape that states
/// its type only through them has nothing for the score function to look at. This
/// is not in SHACL UI, whose score function looks at the shape node alone.
fn first_branch(components: &[ASTComponent]) -> Option<&Object> {
    [true, false].into_iter().find_map(|or| {
        components.iter().find_map(|c| match c {
            ASTComponent::Or(refs) if or => refs.first(),
            ASTComponent::Xone(refs) if !or => refs.first(),
            _ => None,
        })
    })
}

/// The editor of a shape for which SHACL UI's score function returned nothing:
/// a nested form (`shui:DetailsEditor`) if the shape has `sh:node`, a text field
/// (`shui:TextFieldEditor`) otherwise.
///
/// THIS IS NOT PART OF SHACL UI. The Editor's Draft defines the score function's
/// result as possibly empty — "The sequence is empty if no widget matches and is
/// accepted." — and says no more about it. Its default rows need a value to look
/// at ("the value is a blank node"), so for a shape alone they leave a property
/// with only `sh:node`, or with no type fact at all, without an editor, and a form
/// has to show something. The rule is ours, minimal, and reported as
/// `editorSource: "fallback"` so that nobody takes it for the specification's.
fn fallback_editor(components: &[ASTComponent]) -> &'static str {
    if components.iter().any(|c| matches!(c, ASTComponent::Node(_))) {
        editors::DETAILS
    } else {
        editors::TEXT_FIELD
    }
}

fn read_groups(graph: &OxigraphInMemory) -> Vec<PropertyGroupIR> {
    let group_type = format!("{SH}PropertyGroup");
    let mut groups = Vec::new();
    // Collect group subjects (rdf:type sh:PropertyGroup), then their label/order.
    let subjects: Vec<NamedOrBlankNode> = graph
        .quads()
        .filter(|q| q.predicate.as_str() == RDF_TYPE && term_iri(&q.object).as_deref() == Some(group_type.as_str()))
        .map(|q| q.subject.clone())
        .collect();
    for subj in subjects {
        let id = match &subj {
            NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
            NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
        };
        let mut labels = Vec::new();
        let mut order = None;
        for q in graph.quads() {
            if q.subject != subj {
                continue;
            }
            let pred = q.predicate.as_str();
            if pred == format!("{RDFS}label") {
                if let Some(ls) = lang_string(&q.object) {
                    labels.push(ls);
                }
            } else if pred == format!("{SH}order") {
                order = literal_value(&q.object).and_then(|v| v.parse().ok());
            }
        }
        groups.push(PropertyGroupIR { id, labels, order });
    }
    groups
}

fn lang_string(t: &OxTerm) -> Option<LangString> {
    match t {
        OxTerm::Literal(l) => Some(LangString {
            value: l.value().to_string(),
            language: l.language().unwrap_or("").to_string(),
        }),
        _ => None,
    }
}

fn literal_value(t: &OxTerm) -> Option<String> {
    match t {
        OxTerm::Literal(l) => Some(l.value().to_string()),
        _ => None,
    }
}

fn iri_value(t: &OxTerm) -> Option<String> {
    match t {
        OxTerm::NamedNode(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

fn term_iri(t: &OxTerm) -> Option<String> {
    iri_value(t)
}

// ---- path --------------------------------------------------------------------

fn path_to_ir(path: &SHACLPath) -> PathExpr {
    match path {
        SHACLPath::Predicate { pred } => PathExpr::Predicate {
            iri: pred.as_str().to_string(),
        },
        SHACLPath::Inverse { path } => PathExpr::Inverse {
            of: Box::new(path_to_ir(path)),
        },
        SHACLPath::Sequence { paths } => PathExpr::Sequence {
            steps: paths.iter().map(path_to_ir).collect(),
        },
        SHACLPath::Alternative { paths } => PathExpr::Alternative {
            options: paths.iter().map(path_to_ir).collect(),
        },
        SHACLPath::ZeroOrMore { path } => PathExpr::ZeroOrMore {
            path: Box::new(path_to_ir(path)),
        },
        SHACLPath::OneOrMore { path } => PathExpr::OneOrMore {
            path: Box::new(path_to_ir(path)),
        },
        SHACLPath::ZeroOrOne { path } => PathExpr::ZeroOrOne {
            path: Box::new(path_to_ir(path)),
        },
    }
}

// ---- term/value conversions --------------------------------------------------

fn object_iri(o: &Object) -> Option<String> {
    match o {
        Object::Iri(i) => Some(i.as_str().to_string()),
        _ => None,
    }
}

/// Canonical path key matching the TS `pathKey` (used to align projected values
/// to their property shape). Mirrors SPARQL property-path surface syntax.
pub(crate) fn path_key(path: &SHACLPath) -> String {
    match path {
        SHACLPath::Predicate { pred } => pred.as_str().to_string(),
        SHACLPath::Inverse { path } => format!("^{}", path_key(path)),
        SHACLPath::Sequence { paths } => {
            format!("({})", paths.iter().map(path_key).collect::<Vec<_>>().join("/"))
        },
        SHACLPath::Alternative { paths } => {
            format!("({})", paths.iter().map(path_key).collect::<Vec<_>>().join("|"))
        },
        SHACLPath::ZeroOrMore { path } => format!("{}*", path_key(path)),
        SHACLPath::OneOrMore { path } => format!("{}+", path_key(path)),
        SHACLPath::ZeroOrOne { path } => format!("{}?", path_key(path)),
    }
}

pub(crate) fn object_str(o: &Object) -> String {
    match o {
        Object::Iri(i) => i.as_str().to_string(),
        Object::BlankNode(b) => format!("_:{b}"),
        Object::Literal(l) => concrete_lexical(l),
        _ => String::new(),
    }
}

fn iriref_str(iri: &IriRef) -> String {
    match iri {
        IriRef::Iri(i) => i.as_str().to_string(),
        IriRef::Prefixed { prefix, local } => format!("{prefix}:{local}"),
    }
}

fn nodekind_iri(nk: &NodeKind) -> String {
    let local = match nk {
        NodeKind::Iri => "IRI",
        NodeKind::Lit => "Literal",
        NodeKind::BNode => "BlankNode",
        NodeKind::BNodeOrIri => "BlankNodeOrIRI",
        NodeKind::BNodeOrLit => "BlankNodeOrLiteral",
        NodeKind::IriOrLit => "IRIOrLiteral",
        NodeKind::TripleTerm => "TripleTerm",
    };
    format!("{SH}{local}")
}

fn value_to_term(v: &Value) -> TermValue {
    match v {
        Value::Iri(iri) => TermValue::named(&iriref_str(iri)),
        Value::Literal(l) => concrete_to_term(l),
    }
}

fn concrete_to_term(l: &ConcreteLiteral) -> TermValue {
    match l {
        ConcreteLiteral::StringLiteral { lexical_form, lang } => {
            TermValue::literal(lexical_form, None, lang.as_ref().map(|x| x.as_str().to_string()))
        },
        ConcreteLiteral::DatatypeLiteral { lexical_form, datatype }
        | ConcreteLiteral::WrongDatatypeLiteral {
            lexical_form, datatype, ..
        } => TermValue::literal(lexical_form, Some(iriref_str(datatype)), None),
        ConcreteLiteral::NumericLiteral(n) => TermValue::literal(&n.lexical_form(), None, None),
        ConcreteLiteral::DatetimeLiteral(d) => TermValue::literal(&d.to_string(), Some(format!("{XSD}dateTime")), None),
        ConcreteLiteral::BooleanLiteral(b) => TermValue::literal(&b.to_string(), Some(format!("{XSD}boolean")), None),
    }
}

fn concrete_lexical(l: &ConcreteLiteral) -> String {
    match l {
        ConcreteLiteral::StringLiteral { lexical_form, .. }
        | ConcreteLiteral::DatatypeLiteral { lexical_form, .. }
        | ConcreteLiteral::WrongDatatypeLiteral { lexical_form, .. } => lexical_form.clone(),
        ConcreteLiteral::NumericLiteral(n) => n.lexical_form(),
        ConcreteLiteral::DatetimeLiteral(d) => d.to_string(),
        ConcreteLiteral::BooleanLiteral(b) => b.to_string(),
    }
}

fn concrete_f64(l: &ConcreteLiteral) -> Option<f64> {
    concrete_lexical(l).parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudof_lib::form::{FormEngine, RDFFormat, Shapes};
    use wasm_bindgen_test::wasm_bindgen_test;

    const RDF_TYPE_IRI: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

    /// Parse `shapes` (Turtle) through the same façade `Shapes.parse` uses and
    /// project it, so a test sees exactly what JavaScript receives.
    fn model(shapes: &str) -> ShapeModelJson {
        let shapes = Shapes::parse(shapes, &RDFFormat::Turtle, None).expect("shapes parse");
        schema_to_json(shapes.ast(), shapes.graph())
    }

    fn shape<'a>(model: &'a ShapeModelJson, id: &str) -> &'a NodeShapeIR {
        model
            .node_shapes
            .iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("no node shape {id} in the projection"))
    }

    /// `sh:closed` reaches JavaScript, together with the `sh:ignoredProperties`
    /// that make it usable. The projection used to hardcode `closed: None`, so a
    /// closed profile crossed the boundary indistinguishable from an open one.
    #[wasm_bindgen_test]
    fn closed_and_its_ignored_properties_are_projected() {
        let m = model(
            r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix : <http://example.org/> .

:S a sh:NodeShape ;
  sh:targetClass :C ;
  sh:closed true ;
  sh:ignoredProperties ( rdf:type ) ;
  sh:property [ sh:path :p ] .
"#,
        );
        let ns = shape(&m, "http://example.org/S");
        assert_eq!(ns.closed, Some(true), "sh:closed never reached the payload");
        assert_eq!(
            ns.ignored_properties,
            vec![RDF_TYPE_IRI.to_string()],
            "sh:ignoredProperties never reached the payload"
        );
    }

    /// The mirror case: a shape that says nothing about closedness projects
    /// nothing, and neither does one that states `sh:closed false` — both are open
    /// (the DTO skips a `None`, so the key is simply absent for JavaScript).
    #[wasm_bindgen_test]
    fn an_open_shape_projects_no_closed_flag() {
        let m = model(
            r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix : <http://example.org/> .

:Silent a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :p ] .
:Explicit a sh:NodeShape ; sh:targetClass :D ; sh:closed false ; sh:property [ sh:path :p ] .
"#,
        );
        for id in ["http://example.org/Silent", "http://example.org/Explicit"] {
            let ns = shape(&m, id);
            assert_eq!(ns.closed, None, "{id} is open, but projected a closed flag");
            assert!(ns.ignored_properties.is_empty(), "{id} projected exemptions");
        }
    }

    // ---- conditionals ---------------------------------------------------------

    const COND_PREFIXES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix : <http://example.org/> .
"#;

    /// "If the access is restricted, a justification is required", stated with
    /// named shapes. `{OR}` is where the implication goes, so each test writes it
    /// the way it needs.
    fn restricted(or: &str) -> String {
        format!(
            r#"{COND_PREFIXES}
:S a sh:NodeShape ;
  sh:targetClass :Dataset ;
  sh:property [ sh:path :access ] ;
  {or} .

:Restricted a sh:NodeShape ;
  sh:property [ sh:path :access ; sh:hasValue :RESTRICTED ] .

:Justified a sh:NodeShape ;
  sh:property [
    sh:path :justification ;
    sh:minCount 1 ;
    sh:message "Falta la justificación."@es , "Falta la justificació."@ca ;
  ] .
"#
        )
    }

    const JUSTIFICATION: &str = "http://example.org/justification";

    fn then_paths(c: &ConditionalIR) -> Vec<&str> {
        c.then.iter().map(|p| p.path_key.as_str()).collect()
    }

    /// SHACL Core has no conditional component; `sh:or ( [ sh:not C ] T )` is how
    /// one is written. It reaches the payload as a conditional, whichever branch
    /// comes first, and no longer as a disjunction — a consumer that read it as one
    /// would offer "not restricted" as a kind of value.
    #[wasm_bindgen_test]
    fn an_implication_is_a_conditional_in_either_order() {
        for or in [
            "sh:or ( [ sh:not :Restricted ] :Justified )",
            "sh:or ( :Justified [ sh:not :Restricted ] )",
        ] {
            let m = model(&restricted(or));
            let ns = shape(&m, "http://example.org/S");
            assert_eq!(ns.conditionals.len(), 1, "{or}: not read as a conditional");
            let c = &ns.conditionals[0];
            assert_eq!(c.condition_id, "http://example.org/Restricted");
            assert_eq!(c.then_id.as_deref(), Some("http://example.org/Justified"));
            assert_eq!(then_paths(c), vec![JUSTIFICATION]);
            assert!(c.els.is_empty());
            assert!(ns.logical.or.is_none(), "{or}: also projected as a disjunction");
        }
    }

    /// The branches need no names: the ids handed out for anonymous shapes are the
    /// ones `projectForm` and `validateFocus` accept.
    #[wasm_bindgen_test]
    fn an_implication_between_anonymous_shapes_is_a_conditional() {
        let m = model(&format!(
            r#"{COND_PREFIXES}
:S a sh:NodeShape ;
  sh:targetClass :Dataset ;
  sh:or (
    [ sh:not [ sh:property [ sh:path :access ; sh:hasValue :RESTRICTED ] ] ]
    [ sh:property [ sh:path :justification ; sh:minCount 1 ] ]
  ) .
"#
        ));
        let ns = shape(&m, "http://example.org/S");
        assert_eq!(ns.conditionals.len(), 1);
        let c = &ns.conditionals[0];
        assert!(c.condition_id.starts_with("_:"), "condition id: {}", c.condition_id);
        assert!(c.then_id.as_deref().is_some_and(|id| id.starts_with("_:")));
        assert_eq!(then_paths(c), vec![JUSTIFICATION]);
    }

    /// Each `sh:or` on a shape is its own requirement, so two implications are two
    /// conditionals.
    #[wasm_bindgen_test]
    fn two_implications_on_one_shape_are_two_conditionals() {
        let m = model(&format!(
            r#"{}
:S sh:or ( [ sh:not :Open ] :Licensed ) .
:Open a sh:NodeShape ; sh:property [ sh:path :access ; sh:hasValue :PUBLIC ] .
:Licensed a sh:NodeShape ; sh:property [ sh:path :licence ; sh:minCount 1 ] .
"#,
            restricted("sh:or ( [ sh:not :Restricted ] :Justified )")
        ));
        let ns = shape(&m, "http://example.org/S");
        let mut conditions: Vec<&str> = ns.conditionals.iter().map(|c| c.condition_id.as_str()).collect();
        conditions.sort();
        assert_eq!(
            conditions,
            vec!["http://example.org/Open", "http://example.org/Restricted"]
        );
    }

    /// A disjunction of value kinds is not a conditional and stays what it was —
    /// the shape DCAT-AP names once and points a dozen properties at.
    #[wasm_bindgen_test]
    fn a_disjunction_of_value_kinds_stays_a_disjunction() {
        let m = model(&format!(
            r#"{COND_PREFIXES}
:DateOrDateTime a sh:NodeShape ;
  sh:or ( [ sh:datatype xsd:date ] [ sh:datatype xsd:dateTime ] [ sh:datatype xsd:gYear ] ) .
"#
        ));
        let ns = shape(&m, "http://example.org/DateOrDateTime");
        assert!(ns.conditionals.is_empty());
        assert_eq!(ns.logical.or.as_ref().map(Vec::len), Some(3));
    }

    /// What is not exactly an implication is left alone: a negating branch that
    /// also constrains, two negations, a third branch, or a consequent that brings
    /// in no property shape.
    #[wasm_bindgen_test]
    fn what_is_not_exactly_an_implication_stays_a_disjunction() {
        for or in [
            "sh:or ( [ sh:not :Restricted ; sh:nodeKind sh:IRI ] :Justified )",
            "sh:or ( [ sh:not :Restricted ] [ sh:not :Justified ] )",
            "sh:or ( [ sh:not :Restricted ] :Justified [ sh:nodeKind sh:IRI ] )",
            "sh:or ( [ sh:not :Restricted ] [ sh:nodeKind sh:IRI ] )",
        ] {
            let m = model(&restricted(or));
            let ns = shape(&m, "http://example.org/S");
            assert!(ns.conditionals.is_empty(), "{or}: read as a conditional");
            assert!(ns.logical.or.is_some(), "{or}: the disjunction was lost");
        }
    }

    fn session(shapes: &str, data: &str) -> FormEngine {
        let mut engine = FormEngine::new(Shapes::parse(shapes, &RDFFormat::Turtle, None).expect("shapes parse"));
        engine.load_data(data, &RDFFormat::Turtle, None).expect("data parses");
        engine
    }

    fn dataset(triples: &str) -> String {
        format!("@prefix : <http://example.org/> .\n:d a :Dataset {triples} .")
    }

    fn project(engine: &FormEngine) -> ProjectedForm {
        crate::project::project_form(
            engine,
            engine.shapes().ast(),
            &TermValue::named("http://example.org/d"),
            "http://example.org/S",
        )
    }

    /// Whether the condition holds is the validator's call, reported per focus —
    /// and the consequent's values are projected either way, so they are there the
    /// moment it starts to hold.
    #[wasm_bindgen_test]
    fn the_condition_is_satisfied_exactly_when_the_focus_conforms_to_it() {
        let shapes = restricted("sh:or ( [ sh:not :Restricted ] :Justified )");

        let open = project(&session(&shapes, &dataset("; :access :PUBLIC")));
        assert!(open.satisfied.is_empty());
        assert!(open.properties.iter().any(|p| p.path_key == JUSTIFICATION));

        let restricted = project(&session(
            &shapes,
            &dataset("; :access :RESTRICTED ; :justification \"because\""),
        ));
        assert_eq!(restricted.satisfied, vec!["http://example.org/Restricted".to_string()]);
        let justification = restricted
            .properties
            .iter()
            .find(|p| p.path_key == JUSTIFICATION)
            .expect("the consequent's path is projected");
        assert_eq!(justification.values.len(), 1);
    }

    /// Validating the focus against the consequent alone names the path at fault
    /// and carries the author's messages, language tags included. Validating the
    /// disjunction says only that the node failed.
    #[wasm_bindgen_test]
    fn the_consequent_can_be_validated_on_its_own() {
        let shapes = restricted("sh:or ( [ sh:not :Restricted ] :Justified )");
        let engine = session(&shapes, &dataset("; :access :RESTRICTED"));
        let focus = Object::iri(rudof_lib::form::IriS::new_unchecked("http://example.org/d"));

        let whole = engine
            .validate_focus("http://example.org/S", &focus)
            .expect("validates");
        assert_eq!(whole.results.len(), 1);
        assert!(whole.results[0].path().is_none());

        let m = model(&shapes);
        let then_id = shape(&m, "http://example.org/S").conditionals[0]
            .then_id
            .clone()
            .expect("a then id");
        let report = crate::validate::report_from_outcome(&engine.validate_focus(&then_id, &focus).expect("validates"));
        assert_eq!(report.results.len(), 1);
        let result = &report.results[0];
        assert_eq!(result.path_key.as_deref(), Some(JUSTIFICATION));
        let mut languages: Vec<&str> = result.message.iter().map(|m| m.language.as_str()).collect();
        languages.retain(|l| !l.is_empty());
        languages.sort();
        assert_eq!(languages, vec!["ca", "es"]);
    }

    /// The same, when the consequent has no name: the `_:` id the projection hands
    /// out is one the validator resolves.
    #[wasm_bindgen_test]
    fn an_anonymous_consequent_can_be_validated_by_its_id() {
        let shapes = format!(
            r#"{COND_PREFIXES}
:S a sh:NodeShape ;
  sh:targetClass :Dataset ;
  sh:or (
    [ sh:not [ sh:property [ sh:path :access ; sh:hasValue :RESTRICTED ] ] ]
    [ sh:property [ sh:path :justification ; sh:minCount 1 ] ]
  ) .
"#
        );
        let engine = session(&shapes, &dataset("; :access :RESTRICTED"));
        let ast = engine.shapes().ast();
        let graph = engine.shapes().graph();
        let m = schema_to_json(ast, graph);
        let c = &shape(&m, "http://example.org/S").conditionals[0];

        assert_eq!(project(&engine).satisfied, vec![c.condition_id.clone()]);

        let focus = Object::iri(rudof_lib::form::IriS::new_unchecked("http://example.org/d"));
        let outcome = engine
            .validate_focus(c.then_id.as_deref().expect("a then id"), &focus)
            .expect("an anonymous shape id resolves");
        assert_eq!(outcome.results.len(), 1);
        assert!(outcome.results[0].path().is_some());
    }

    // ---- sh:targetWhere -------------------------------------------------------

    /// The same requirement as `restricted`, in SHACL 1.2: the consequent is the
    /// shape with the where target, and no shape mentions the other.
    fn restricted_where() -> String {
        format!(
            r#"{COND_PREFIXES}
:S a sh:NodeShape ;
  sh:targetClass :Dataset ;
  sh:property [ sh:path :access ] .

:Restricted a sh:NodeShape ;
  sh:property [ sh:path :access ; sh:hasValue :RESTRICTED ] .

:Justified a sh:NodeShape ;
  sh:targetWhere :Restricted ;
  sh:property [
    sh:path :justification ;
    sh:minCount 1 ;
    sh:message "Falta la justificación."@es , "Falta la justificació."@ca ;
  ] .
"#
        )
    }

    /// `sh:targetWhere C` on T is the conditional "when C, T", available to every
    /// node shape but T and C themselves; it is not a disjunction.
    #[wasm_bindgen_test]
    fn a_where_target_is_a_conditional_of_every_other_node_shape() {
        let m = model(&restricted_where());
        let ns = shape(&m, "http://example.org/S");
        assert_eq!(ns.conditionals.len(), 1);
        let c = &ns.conditionals[0];
        assert_eq!(c.condition_id, "http://example.org/Restricted");
        assert_eq!(c.then_id.as_deref(), Some("http://example.org/Justified"));
        assert_eq!(then_paths(c), vec![JUSTIFICATION]);
        assert!(c.els.is_empty());
        assert!(shape(&m, "http://example.org/Justified").conditionals.is_empty());
        assert!(shape(&m, "http://example.org/Restricted").conditionals.is_empty());
    }

    /// An anonymous where shape is a condition all the same.
    #[wasm_bindgen_test]
    fn an_anonymous_where_shape_is_a_condition() {
        let m = model(&format!(
            r#"{COND_PREFIXES}
:S a sh:NodeShape ; sh:targetClass :Dataset .
:Justified a sh:NodeShape ;
  sh:targetWhere [ sh:property [ sh:path :access ; sh:hasValue :RESTRICTED ] ] ;
  sh:property [ sh:path :justification ; sh:minCount 1 ] .
"#
        ));
        let c = &shape(&m, "http://example.org/S").conditionals[0];
        assert!(c.condition_id.starts_with("_:"), "condition id: {}", c.condition_id);
        assert_eq!(then_paths(c), vec![JUSTIFICATION]);
    }

    /// The requirement stated both ways is one conditional, not two.
    #[wasm_bindgen_test]
    fn the_same_requirement_stated_both_ways_is_one_conditional() {
        let shapes = format!(
            "{}\n:Justified sh:targetWhere :Restricted .",
            restricted("sh:or ( [ sh:not :Restricted ] :Justified )")
        );
        let m = model(&shapes);
        assert_eq!(shape(&m, "http://example.org/S").conditionals.len(), 1);
    }

    /// Projection and validation by id behave as for the implication.
    #[wasm_bindgen_test]
    fn a_where_conditional_is_satisfied_and_validated_like_an_implication() {
        let shapes = restricted_where();

        let open = project(&session(&shapes, &dataset("; :access :PUBLIC")));
        assert!(open.satisfied.is_empty());
        assert!(open.properties.iter().any(|p| p.path_key == JUSTIFICATION));

        let engine = session(&shapes, &dataset("; :access :RESTRICTED"));
        assert_eq!(
            project(&engine).satisfied,
            vec!["http://example.org/Restricted".to_string()]
        );

        let focus = Object::iri(rudof_lib::form::IriS::new_unchecked("http://example.org/d"));
        let report = crate::validate::report_from_outcome(
            &engine
                .validate_focus("http://example.org/Justified", &focus)
                .expect("validates"),
        );
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].path_key.as_deref(), Some(JUSTIFICATION));
    }

    /// A form for a class whose nodes can never conform to the where shape still
    /// works: the conditional is there and is never satisfied.
    #[wasm_bindgen_test]
    fn a_form_that_can_never_meet_the_where_shape_is_unaffected() {
        let shapes = format!(
            "{}\n:Other a sh:NodeShape ; sh:targetClass :Thing ; sh:property [ sh:path :label ] .",
            restricted_where()
        );
        let engine = session(
            &shapes,
            "@prefix : <http://example.org/> . :t a :Thing ; :label \"x\" .",
        );
        let form = crate::project::project_form(
            &engine,
            engine.shapes().ast(),
            &TermValue::named("http://example.org/t"),
            "http://example.org/Other",
        );
        assert!(form.satisfied.is_empty());
        assert!(form.properties.iter().any(|p| p.path_key == "http://example.org/label"));
    }
}
