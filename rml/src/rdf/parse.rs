//! Reading a mapping from its graph.
//!
//! A mapping component is "defined by the presence or absence of certain
//! properties" and typing it is "OPTIONAL and has no effect on the behaviour of
//! an RML processor" (RML-Core §3.2), so the parser reads properties and takes
//! `rdf:type` as a hint at most. The constant shortcuts (§8.1.1, §9.2.1) and
//! their long forms are read into the same value.
//!
//! A property in the RML namespace that the AST does not model is
//! [`Error::Unsupported`], never ignored: a graph map left out would put
//! triples in the wrong graph, and a datatype map left out would change a
//! literal. A property outside the namespace on a mapping component is
//! documentation (§3.2) and is ignored.

use crate::Error;
use crate::ast::{
    Annotation, Constant, Expression, Iterable, JoinCondition, LanguageMap, Literal, LiteralKind, LogicalSource,
    Mapping, Node, ObjectMap, Position, PredicateObjectMap, RefObjectMap, SubjectMap, Template, TermMap, TermType,
    TriplesMap,
};
use crate::vocab::{self, is_rml, rdf, rml, term as t, xsd};
use rudof_iri::IriS;
use rudof_rdf::backend::{OxigraphInMemory, OxigraphInMemoryError, ReaderMode};
use rudof_rdf::term::{Object, Triple, literal::ConcreteLiteral};
use rudof_rdf::{NeighsRDF, RDFFormat, STRING_BASE};
use std::collections::{BTreeSet, HashMap};

/// The mapping of a Turtle document (RML-Core §3.3), its relative IRIs
/// resolved against `base`.
pub fn parse_turtle(text: &str, base: Option<&str>) -> Result<Mapping, Error> {
    let graph = OxigraphInMemory::from_str(
        text,
        &RDFFormat::Turtle,
        Some(base.unwrap_or(STRING_BASE)),
        &ReaderMode::Strict,
    )
    .map_err(|e| match e {
        OxigraphInMemoryError::Syntax { error, .. } => Error::Graph(error.to_string()),
        e => Error::Graph(e.to_string()),
    })?;
    parse(&graph)
}

/// The mapping a graph holds: every triples map in it, in the canonical order
/// ([`Mapping::canonical`]), and the annotations about the IRIs it names.
pub fn parse<R: NeighsRDF>(source: &R) -> Result<Mapping, Error> {
    let graph = Graph::of(source)?;
    let mut ids = BTreeSet::new();
    for (subject, arcs) in &graph.arcs {
        let component = arcs.iter().any(|(p, o)| {
            [
                t::P_LOGICAL_SOURCE,
                t::P_SUBJECT_MAP,
                t::P_SUBJECT,
                t::P_PREDICATE_OBJECT_MAP,
            ]
            .iter()
            .any(|local| *p == rml(local))
                || (*p == rdf("type") && *o == Object::iri(rml(t::TRIPLES_MAP)))
        });
        if component {
            ids.insert(node(subject)?);
        }
    }
    let mut triples_maps = Vec::new();
    for id in &ids {
        triples_maps.push(triples_map(&graph, id)?);
    }
    let mut mapping = Mapping {
        triples_maps,
        annotations: Vec::new(),
    };
    mapping.annotations = annotations(&graph, &mapping, &ids);
    mapping.validate()?;
    Ok(mapping.canonical())
}

/// The triples about a class or a predicate the mapping names, outside the
/// RML vocabulary (RML-Core §3.2). A value that is not an IRI or a literal is
/// not carried.
fn annotations(graph: &Graph, mapping: &Mapping, ids: &BTreeSet<Node>) -> Vec<Annotation> {
    let mut named: BTreeSet<IriS> = BTreeSet::new();
    for m in &mapping.triples_maps {
        named.extend(m.subject_map.classes.iter().cloned());
        for pom in &m.predicate_object_maps {
            for p in &pom.predicates {
                if let Expression::Constant(Constant::Iri(iri)) = &p.expression {
                    named.insert(iri.clone());
                }
            }
        }
    }
    let mut out = Vec::new();
    for subject in named {
        if ids.contains(&Node::Iri(subject.clone())) {
            continue;
        }
        let Some(arcs) = graph.arcs.get(&Object::iri(subject.clone())) else {
            continue;
        };
        for (predicate, object) in arcs {
            if is_rml(predicate) {
                continue;
            }
            if let Ok(object) = constant(object, "") {
                out.push(Annotation {
                    subject: subject.clone(),
                    predicate: predicate.clone(),
                    object,
                });
            }
        }
    }
    out
}

// ---- the graph, as arcs by subject ----------------------------------------------

struct Graph {
    arcs: HashMap<Object, Vec<(IriS, Object)>>,
}

impl Graph {
    fn of<R: NeighsRDF>(source: &R) -> Result<Self, Error> {
        let graph_error = |e: &dyn std::fmt::Display| Error::Graph(e.to_string());
        let mut arcs: HashMap<Object, Vec<(IriS, Object)>> = HashMap::new();
        for triple in source.triples().map_err(|e| graph_error(&e))? {
            let (s, p, o) = triple.into_components();
            let s = R::subject_as_node(&s).map_err(|e| graph_error(&e))?;
            let p: IriS = p.into();
            let o = R::term_as_object(&o).map_err(|e| graph_error(&e))?;
            arcs.entry(s).or_default().push((p, o));
        }
        Ok(Graph { arcs })
    }
}

/// The properties of one resource, and which of them were read.
struct Props<'g> {
    at: String,
    arcs: &'g [(IriS, Object)],
    used: Vec<bool>,
}

impl<'g> Props<'g> {
    fn of(graph: &'g Graph, resource: &Object, at: String) -> Result<Self, Error> {
        if !matches!(resource, Object::Iri(_) | Object::BlankNode(_)) {
            return Err(Error::invalid(
                &at,
                "a mapping component is an IRI or a blank node, not a literal",
            ));
        }
        let arcs = graph.arcs.get(resource).map(Vec::as_slice).unwrap_or(&[]);
        Ok(Props {
            at,
            arcs,
            used: vec![false; arcs.len()],
        })
    }

    fn all(&mut self, local: &str) -> Vec<&'g Object> {
        let property = rml(local);
        let mut out = Vec::new();
        for (i, (p, o)) in self.arcs.iter().enumerate() {
            if *p == property {
                self.used[i] = true;
                out.push(o);
            }
        }
        out
    }

    fn one(&mut self, local: &str) -> Result<Option<&'g Object>, Error> {
        let values = self.all(local);
        match values.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one)),
            _ => Err(Error::invalid(
                &self.at,
                &format!("more than one rml:{local}, which has at most one"),
            )),
        }
    }

    fn required(&mut self, local: &str) -> Result<&'g Object, Error> {
        self.one(local)?
            .ok_or_else(|| Error::invalid(&self.at, &format!("rml:{local} is required")))
    }

    /// Refuses a property of the RML vocabulary that was not read.
    fn finish(self) -> Result<(), Error> {
        for ((p, _), used) in self.arcs.iter().zip(&self.used) {
            if !used && is_rml(p) {
                let local = &p.as_str()[vocab::RML.len()..];
                return Err(Error::unsupported(&self.at, &format!("rml:{local} here")));
            }
        }
        Ok(())
    }
}

// ---- values ---------------------------------------------------------------------

fn node(o: &Object) -> Result<Node, Error> {
    match o {
        Object::Iri(i) => Ok(Node::Iri(i.clone())),
        Object::BlankNode(b) => Ok(Node::Blank(b.clone())),
        other => Err(Error::invalid(&other.to_string(), "expected an IRI or a blank node")),
    }
}

fn iri(o: &Object, at: &str) -> Result<IriS, Error> {
    match o {
        Object::Iri(i) => Ok(i.clone()),
        other => Err(Error::invalid(at, &format!("expected an IRI, found {other}"))),
    }
}

fn string(o: &Object, at: &str) -> Result<String, Error> {
    if let Object::Literal(l) = o
        && l.lang().is_none()
        && l.datatype().get_iri().is_ok_and(|d| *d == xsd("string"))
    {
        return Ok(l.lexical_form());
    }
    Err(Error::invalid(at, &format!("expected a string literal, found {o}")))
}

fn literal(l: &ConcreteLiteral, at: &str) -> Result<Literal, Error> {
    let kind = match l.lang() {
        Some(tag) => LiteralKind::Language(tag.as_str().to_owned()),
        None => LiteralKind::Datatype(
            l.datatype()
                .get_iri()
                .map_err(|e| Error::invalid(at, &e.to_string()))?
                .clone(),
        ),
    };
    Ok(Literal {
        lexical_form: l.lexical_form(),
        kind,
    })
}

/// A constant term (§8.1): an IRI or a literal.
fn constant(o: &Object, at: &str) -> Result<Constant, Error> {
    match o {
        Object::Iri(i) => Ok(Constant::Iri(i.clone())),
        Object::Literal(l) => Ok(Constant::Literal(literal(l, at)?)),
        other => Err(Error::invalid(
            at,
            &format!("a constant is an IRI or a literal, found {other} (RML-Core §8.4.2)"),
        )),
    }
}

// ---- components -----------------------------------------------------------------

fn triples_map(graph: &Graph, id: &Node) -> Result<TriplesMap, Error> {
    let at = id.to_string();
    let subject = match id {
        Node::Iri(i) => Object::iri(i.clone()),
        Node::Blank(b) => Object::bnode(b.clone()),
    };
    let mut props = Props::of(graph, &subject, at.clone())?;
    let logical_source = logical_source(
        graph,
        props.required(t::P_LOGICAL_SOURCE).map_err(|_| {
            Error::unsupported(
                &at,
                "a triples map without a logical source; its term maps would have no iteration",
            )
        })?,
        &at,
    )?;
    let subject_map = match (props.one(t::P_SUBJECT_MAP)?, props.one(t::P_SUBJECT)?) {
        (Some(map), None) => {
            let (term, classes) = term_map(graph, map, Position::Subject, &format!("{at} subject map"))?;
            SubjectMap { term, classes }
        },
        (None, Some(constant_subject)) => SubjectMap {
            term: TermMap::iri(iri(constant_subject, &format!("{at} rml:subject"))?),
            classes: Vec::new(),
        },
        _ => {
            // §6: "it MUST have exactly one subject map".
            return Err(Error::invalid(
                &at,
                "a triples map has exactly one subject map (rml:subjectMap or rml:subject)",
            ));
        },
    };
    let mut predicate_object_maps = Vec::new();
    for pom in props.all(t::P_PREDICATE_OBJECT_MAP) {
        predicate_object_maps.push(predicate_object_map(graph, pom, &at)?);
    }
    props.finish()?;
    Ok(TriplesMap {
        id: id.clone(),
        logical_source,
        subject_map,
        predicate_object_maps,
    })
}

fn logical_source(graph: &Graph, o: &Object, parent: &str) -> Result<LogicalSource, Error> {
    let at = format!("{parent} logical source");
    let mut props = Props::of(graph, o, at.clone())?;
    // RML-IO §3.1: "exactly one source (`rml:source`)".
    let source = match props.required(t::P_SOURCE)? {
        Object::Iri(i) => Some(i.clone()),
        Object::BlankNode(_) => None,
        other => return Err(Error::invalid(&at, &format!("rml:source is a resource, found {other}"))),
    };
    let formulation = props.required(t::P_REFERENCE_FORMULATION).map_err(|_| {
        // RML-IO §3.1: "The reference formulation MUST be specified in the case that the data source is a relational database".
        Error::invalid(&at, "rml:referenceFormulation is required for a relational source")
    })?;
    let iterator = string(props.required(t::P_ITERATOR)?, &format!("{at} rml:iterator"))?;
    props.finish()?;
    let iterable = if *formulation == Object::iri(rml(t::SQL2008_TABLE)) {
        Iterable::Sql2008Table(iterator)
    } else if *formulation == Object::iri(rml(t::SQL2008_QUERY)) {
        Iterable::Sql2008Query(iterator)
    } else {
        return Err(Error::unsupported(
            &at,
            &format!("the reference formulation {formulation}; only SQL sources are"),
        ));
    };
    Ok(LogicalSource { source, iterable })
}

fn predicate_object_map(graph: &Graph, o: &Object, parent: &str) -> Result<PredicateObjectMap, Error> {
    let at = format!("{parent} predicate-object map");
    let mut props = Props::of(graph, o, at.clone())?;
    let mut predicates = Vec::new();
    for p in props.all(t::P_PREDICATE) {
        predicates.push(TermMap::iri(iri(p, &format!("{at} rml:predicate"))?));
    }
    for p in props.all(t::P_PREDICATE_MAP) {
        predicates.push(term_map(graph, p, Position::Predicate, &format!("{at} predicate map"))?.0);
    }
    let mut objects = Vec::new();
    for c in props.all(t::P_OBJECT) {
        objects.push(ObjectMap::Term(TermMap {
            expression: Expression::Constant(constant(c, &format!("{at} rml:object"))?),
            term_type: None,
            datatype: None,
            language: None,
        }));
    }
    for m in props.all(t::P_OBJECT_MAP) {
        objects.push(object_map(graph, m, &format!("{at} object map"))?);
    }
    props.finish()?;
    Ok(PredicateObjectMap { predicates, objects })
}

fn object_map(graph: &Graph, o: &Object, at: &str) -> Result<ObjectMap, Error> {
    let mut probe = Props::of(graph, o, at.to_owned())?;
    if probe.one(t::P_PARENT_TRIPLES_MAP)?.is_none() {
        return Ok(ObjectMap::Term(term_map(graph, o, Position::Object, at)?.0));
    }
    let at = format!("{at} (referencing)");
    let mut props = Props::of(graph, o, at.clone())?;
    let parent = node(props.required(t::P_PARENT_TRIPLES_MAP)?)?;
    let mut join_conditions = Vec::new();
    for jc in props.all(t::P_JOIN_CONDITION) {
        join_conditions.push(join_condition(graph, jc, &at)?);
    }
    props.finish()?;
    Ok(ObjectMap::Reference(RefObjectMap {
        parent,
        join_conditions,
    }))
}

fn join_condition(graph: &Graph, o: &Object, parent: &str) -> Result<JoinCondition, Error> {
    let at = format!("{parent} join condition");
    let mut props = Props::of(graph, o, at.clone())?;
    // §9.2: "exactly one value for each" of a child map and a parent map; §9.2.1 their reference shortcuts.
    let mut side = |shortcut: &str, long: &str| -> Result<String, Error> {
        let short = props.one(shortcut)?;
        let map = props.one(long)?;
        match (short, map) {
            (Some(r), None) => string(r, &format!("{at} rml:{shortcut}")),
            (None, Some(m)) => {
                let mut inner = Props::of(graph, m, format!("{at} rml:{long}"))?;
                let r = inner.one(t::P_REFERENCE)?;
                let Some(r) = r else {
                    return Err(Error::unsupported(
                        &at,
                        &format!("a join condition whose rml:{long} is not a reference"),
                    ));
                };
                let column = string(r, &format!("{at} rml:{long}"))?;
                inner.finish()?;
                Ok(column)
            },
            _ => Err(Error::invalid(
                &at,
                &format!("a join condition has exactly one rml:{shortcut} or rml:{long}"),
            )),
        }
    };
    let child = side(t::P_CHILD, t::P_CHILD_MAP)?;
    let parent_column = side(t::P_PARENT, t::P_PARENT_MAP)?;
    props.finish()?;
    Ok(JoinCondition {
        child,
        parent: parent_column,
    })
}

/// A term map, and the classes it carries (a subject map's `rml:class`, §6.2).
fn term_map(graph: &Graph, o: &Object, position: Position, at: &str) -> Result<(TermMap, Vec<IriS>), Error> {
    let mut props = Props::of(graph, o, at.to_owned())?;
    let constant_value = props.one(t::P_CONSTANT)?;
    let reference = props.one(t::P_REFERENCE)?;
    let template = props.one(t::P_TEMPLATE)?;
    let expression = match (constant_value, reference, template) {
        (Some(c), None, None) => Expression::Constant(constant(c, at)?),
        (None, Some(r), None) => Expression::Reference(string(r, &format!("{at} rml:reference"))?),
        (None, None, Some(tpl)) => Expression::Template(
            Template::parse(&string(tpl, &format!("{at} rml:template"))?)
                .map_err(|reason| Error::invalid(&format!("{at} rml:template"), &reason))?,
        ),
        (None, None, None) => {
            return Err(Error::unsupported(
                at,
                "a term map with no expression (a blank node without one is a fresh node per iteration)",
            ));
        },
        // §7.1: an expression map has one of constant, reference, template.
        _ => {
            return Err(Error::invalid(
                at,
                "a term map has one of rml:constant, rml:reference, rml:template",
            ));
        },
    };
    let term_type = props.one(t::P_TERM_TYPE)?.map(|o| term_type(o, at)).transpose()?;
    let datatype = match (props.one(t::P_DATATYPE)?, props.one(t::P_DATATYPE_MAP)?) {
        (None, None) => None,
        (Some(d), None) => Some(iri(d, &format!("{at} rml:datatype"))?),
        (None, Some(map)) => {
            let mut inner = Props::of(graph, map, format!("{at} rml:datatypeMap"))?;
            let Some(c) = inner.one(t::P_CONSTANT)? else {
                return Err(Error::unsupported(
                    at,
                    "a datatype map that is not constant (RML-Core §8.6)",
                ));
            };
            let datatype = iri(c, &format!("{at} rml:datatypeMap"))?;
            inner.finish()?;
            Some(datatype)
        },
        (Some(_), Some(_)) => return Err(Error::invalid(at, "a term map has at most one datatype map")),
    };
    let language = match (props.one(t::P_LANGUAGE)?, props.one(t::P_LANGUAGE_MAP)?) {
        (None, None) => None,
        (Some(l), None) => Some(LanguageMap::Constant(string(l, &format!("{at} rml:language"))?)),
        (None, Some(map)) => {
            let mut inner = Props::of(graph, map, format!("{at} rml:languageMap"))?;
            let language = match (inner.one(t::P_REFERENCE)?, inner.one(t::P_CONSTANT)?) {
                (Some(r), None) => LanguageMap::Reference(string(r, &format!("{at} rml:languageMap"))?),
                (None, Some(c)) => LanguageMap::Constant(string(c, &format!("{at} rml:languageMap"))?),
                (None, None) => {
                    return Err(Error::unsupported(at, "a language map that is a template"));
                },
                _ => {
                    return Err(Error::invalid(
                        at,
                        "a language map has one of rml:reference, rml:constant",
                    ));
                },
            };
            inner.finish()?;
            Some(language)
        },
        (Some(_), Some(_)) => return Err(Error::invalid(at, "a term map has at most one language map")),
    };
    let mut classes = Vec::new();
    for class in props.all(t::P_CLASS) {
        classes.push(iri(class, &format!("{at} rml:class"))?);
    }
    if !classes.is_empty() && position != Position::Subject {
        return Err(Error::invalid(at, "rml:class belongs to a subject map (RML-Core §6.2)"));
    }
    props.finish()?;
    Ok((
        TermMap {
            expression,
            term_type,
            datatype,
            language,
        },
        classes,
    ))
}

fn term_type(o: &Object, at: &str) -> Result<TermType, Error> {
    let iri = iri(o, &format!("{at} rml:termType"))?;
    let local = iri.as_str().strip_prefix(vocab::RML).unwrap_or("");
    match local {
        t::IRI => Ok(TermType::Iri),
        t::UNSAFE_IRI => Ok(TermType::UnsafeIri),
        t::BLANK_NODE => Ok(TermType::BlankNode),
        t::LITERAL => Ok(TermType::Literal),
        "URI" | "UnsafeURI" => Err(Error::unsupported(at, &format!("the term type rml:{local}"))),
        _ => Err(Error::invalid(
            at,
            &format!("{iri} is not an RML term type (§8.4)", iri = iri.as_str()),
        )),
    }
}
