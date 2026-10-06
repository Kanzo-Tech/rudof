//! Reads an RML mapping into [`Tables`].
//!
//! The mapping is RML 2.0 — RML-Core, RML-IO and RML-LV of the W3C KG-Construct
//! Community Group (namespace `http://w3id.org/rml/`), the dialect RMLio's
//! MappingLoom reads — restricted to what a relational schema needs:
//!
//! - **Sources** (RML-IO §3.1): an `rml:LogicalSource` over a SQL table, by
//!   name: `rml:referenceFormulation rml:SQL2008Table` with the table in
//!   `rml:iterator`. Its `rml:source` says how to reach the database, which is
//!   the host's business: it is required, and not read.
//! - **Views** (RML-LV §3-5): an `rml:LogicalView` `rml:viewOn` a source or
//!   another view, with expression fields (`rml:fieldName` + `rml:reference`)
//!   and `rml:innerJoin` / `rml:leftJoin` to parent views
//!   (`rml:parentLogicalView`, `rml:joinCondition`, `rml:field`).
//! - **Term maps** (RML-Core §8): `rml:reference` or `rml:constant` (and the
//!   `rml:subject`, `rml:predicate`, `rml:object`, `rml:datatype`,
//!   `rml:language` shortcuts), with `rml:termType`, `rml:datatype(Map)`,
//!   `rml:language(Map)`, and `rml:class` on subject maps; `rml:baseIRI` on a
//!   triples map resolves relative IRIs.
//! - **Joins** (RML-Core §9): `rml:parentTriplesMap` with `rml:joinCondition`
//!   (`rml:child`/`rml:parent` or `rml:childMap`/`rml:parentMap`), or none
//!   when both triples maps read the same source.
//!
//! Every other RML term — templates, graph maps, SQL queries, JSON or CSV
//! sources, iterable fields, … — is refused with
//! [`SqlCompileError::UnsupportedRml`] naming it, never ignored. Terms outside
//! the RML namespace (labels, comments) have no effect on the mapping and
//! are allowed.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{parse_identifier, parse_object_name};
use crate::validator::sql::dialect::Dialect;
use crate::validator::sql::tables::{ObjectRule, Rule, Source, Tables, TermRule, TermType, Value, View, ViewJoin};
use crate::validator::sql::term::encode;
use oxrdf::{NamedOrBlankNode, Term};
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::term::Triple;
use rudof_rdf::{NeighsRDF, RDFFormat};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

const RML: &str = "http://w3id.org/rml/";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
/// The base a mapping document's own relative IRIs (`<#TriplesMap>`) resolve
/// against; it names mapping nodes only and never reaches a generated term.
const DOCUMENT_BASE: &str = "file:///mapping.rml.ttl";

fn rml(local: &str) -> String {
    format!("{RML}{local}")
}

fn refuse(term: &str, why: &str) -> SqlCompileError {
    SqlCompileError::UnsupportedRml(format!("{term}: {why}"))
}

fn malformed(message: String) -> SqlCompileError {
    SqlCompileError::Mapping(message)
}

/// The mapping graph, indexed by subject.
struct Doc {
    arcs: BTreeMap<String, Vec<(String, Term)>>,
    /// Every subject, by its key.
    nodes: BTreeMap<String, Term>,
}

/// A node of the mapping graph, by its N-Triples text.
fn key(term: &Term) -> String {
    term.to_string()
}

fn short(iri: &str) -> String {
    match iri.strip_prefix(RML) {
        Some(local) => format!("rml:{local}"),
        None => format!("<{iri}>"),
    }
}

impl Doc {
    fn parse(turtle: &str) -> Result<Self, SqlCompileError> {
        let graph = OxigraphInMemory::from_str(turtle, &RDFFormat::Turtle, Some(DOCUMENT_BASE), &ReaderMode::Strict)
            .map_err(|e| malformed(format!("the RML mapping does not parse as Turtle: {e}")))?;
        let mut arcs: BTreeMap<String, Vec<(String, Term)>> = BTreeMap::new();
        let mut nodes = BTreeMap::new();
        let triples = graph
            .triples()
            .map_err(|e| malformed(format!("reading the RML mapping: {e}")))?;
        for triple in triples {
            let (subject, predicate, object) = triple.into_components();
            let subject: Term = match subject {
                NamedOrBlankNode::NamedNode(n) => n.into(),
                NamedOrBlankNode::BlankNode(b) => b.into(),
            };
            arcs.entry(key(&subject))
                .or_default()
                .push((predicate.as_str().to_owned(), object));
            nodes.insert(key(&subject), subject);
        }
        Ok(Self { arcs, nodes })
    }

    fn values(&self, node: &Term, predicate: &str) -> Vec<&Term> {
        self.arcs
            .get(&key(node))
            .into_iter()
            .flatten()
            .filter(|(p, _)| p == predicate)
            .map(|(_, o)| o)
            .collect()
    }

    fn one(&self, node: &Term, predicate: &str) -> Result<Option<&Term>, SqlCompileError> {
        let values = self.values(node, predicate);
        match values.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one)),
            _ => Err(malformed(format!("{node} has more than one {}", short(predicate)))),
        }
    }

    fn required(&self, node: &Term, predicate: &str) -> Result<&Term, SqlCompileError> {
        self.one(node, predicate)?
            .ok_or_else(|| malformed(format!("{node} has no {}", short(predicate))))
    }

    fn string(&self, node: &Term, predicate: &str) -> Result<Option<String>, SqlCompileError> {
        match self.one(node, predicate)? {
            None => Ok(None),
            Some(Term::Literal(l)) => Ok(Some(l.value().to_owned())),
            Some(other) => Err(malformed(format!(
                "{} of {node} is {other}, not a string",
                short(predicate)
            ))),
        }
    }

    /// A reference (`rml:reference`, `rml:child`, …): a SQL identifier,
    /// delimited or not (R2RML §6, §10 identifier rules), by its value.
    fn reference(&self, node: &Term, predicate: &str) -> Result<Option<String>, SqlCompileError> {
        match self.string(node, predicate)? {
            None => Ok(None),
            Some(text) => parse_identifier(&text)
                .map(|ident| Some(ident.value))
                .map_err(|e| malformed(format!("{} of {node}: {e}", short(predicate)))),
        }
    }

    /// What tells two `rml:Source`s apart: an IRI by itself, a blank node by
    /// its description (two `[ a rml:Source ]` are the same source).
    fn signature(&self, node: &Term) -> String {
        match node {
            Term::BlankNode(_) => {
                let mut arcs: Vec<String> = self
                    .arcs
                    .get(&key(node))
                    .into_iter()
                    .flatten()
                    .map(|(p, o)| format!("{p} {o}"))
                    .collect();
                arcs.sort();
                format!("[{}]", arcs.join("; "))
            },
            other => key(other),
        }
    }

    fn types(&self, node: &Term) -> BTreeSet<String> {
        self.values(node, RDF_TYPE)
            .into_iter()
            .filter_map(|t| match t {
                Term::NamedNode(n) => Some(n.as_str().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// Refuses every RML property of `node` outside `allowed`.
    fn only(&self, node: &Term, allowed: &[&str], role: &str) -> Result<(), SqlCompileError> {
        for (predicate, _) in self.arcs.get(&key(node)).into_iter().flatten() {
            if let Some(local) = predicate.strip_prefix(RML)
                && !allowed.contains(&local)
            {
                return Err(refuse(&format!("rml:{local}"), &format!("not supported on a {role}")));
            }
        }
        Ok(())
    }
}

/// Reads RML into the [`Tables`] model.
struct Reader<'d> {
    doc: &'d Doc,
    /// The distinct `rml:Source`s read, by [`Doc::signature`].
    sources: RefCell<BTreeSet<String>>,
}

/// What a triples map reads, and how it names its subject.
struct TriplesMap {
    source: Source,
    subject: TermRule,
    classes: Vec<String>,
    base_iri: Option<String>,
}

impl Reader<'_> {
    fn triples_maps(&self) -> Vec<Term> {
        self.doc
            .arcs
            .iter()
            .filter(|(_, arcs)| {
                arcs.iter().any(|(p, o)| {
                    (p == RDF_TYPE && matches!(o, Term::NamedNode(n) if n.as_str() == rml("TriplesMap")))
                        || *p == rml("subjectMap")
                        || *p == rml("subject")
                })
            })
            .filter_map(|(node, _)| self.doc.nodes.get(node).cloned())
            .collect()
    }

    fn triples_map(&self, node: &Term) -> Result<TriplesMap, SqlCompileError> {
        self.doc.only(
            node,
            &[
                "logicalSource",
                "subjectMap",
                "subject",
                "predicateObjectMap",
                "baseIRI",
            ],
            "triples map",
        )?;
        let source = match self.doc.one(node, &rml("logicalSource"))? {
            Some(source) => self.source(source)?,
            None => Source::Unit,
        };
        let base_iri = match self.doc.one(node, &rml("baseIRI"))? {
            Some(Term::NamedNode(n)) => Some(n.as_str().to_owned()),
            Some(other) => return Err(malformed(format!("rml:baseIRI {other} is not an IRI"))),
            None => None,
        };
        let subject_maps = self.doc.values(node, &rml("subjectMap"));
        let subject_constants = self.doc.values(node, &rml("subject"));
        let (subject, classes) = match (subject_maps.as_slice(), subject_constants.as_slice()) {
            ([map], []) => {
                self.doc
                    .only(map, &["reference", "constant", "termType", "class"], "subject map")?;
                let rule = self.term_map(map, TermPosition::Subject, base_iri.as_deref())?;
                let mut classes = Vec::new();
                for class in self.doc.values(map, &rml("class")) {
                    match class {
                        Term::NamedNode(n) => classes.push(n.as_str().to_owned()),
                        other => return Err(malformed(format!("rml:class {other} is not an IRI"))),
                    }
                }
                (rule, classes)
            },
            ([], [constant]) => (Self::constant(constant)?, Vec::new()),
            _ => return Err(malformed(format!("{node} must have exactly one subject map"))),
        };
        Ok(TriplesMap {
            source,
            subject,
            classes,
            base_iri,
        })
    }

    fn source(&self, node: &Term) -> Result<Source, SqlCompileError> {
        let types = self.doc.types(node);
        if types.contains(&rml("LogicalView")) || !self.doc.values(node, &rml("viewOn")).is_empty() {
            return Ok(Source::View(Box::new(self.view(node)?)));
        }
        self.doc
            .only(node, &["source", "referenceFormulation", "iterator"], "logical source")?;
        let source = self.doc.required(node, &rml("source"))?;
        self.sources.borrow_mut().insert(self.doc.signature(source));
        match self.doc.required(node, &rml("referenceFormulation"))? {
            Term::NamedNode(n) if n.as_str() == rml("SQL2008Table") => {},
            Term::NamedNode(n) => {
                return Err(refuse(
                    &short(n.as_str()),
                    "only rml:SQL2008Table logical sources are read",
                ));
            },
            other => return Err(malformed(format!("rml:referenceFormulation {other} is not an IRI"))),
        }
        let table = self
            .doc
            .string(node, &rml("iterator"))?
            .ok_or_else(|| malformed(format!("{node} names no table (rml:iterator)")))?;
        // RML-IO §3.1: a SQL table name, delimited and qualified as SQL allows.
        let table = parse_object_name(&table).map_err(|e| malformed(format!("the table of {node}: {e}")))?;
        Ok(Source::Table(table))
    }

    fn view(&self, node: &Term) -> Result<View, SqlCompileError> {
        self.doc
            .only(node, &["viewOn", "field", "innerJoin", "leftJoin"], "logical view")?;
        let on = self.source(self.doc.required(node, &rml("viewOn"))?)?;
        let fields = self.fields(node, "logical view")?;
        if fields.is_empty() {
            return Err(malformed(format!("the logical view {node} has no rml:field")));
        }
        let mut joins = Vec::new();
        for (property, left) in [("innerJoin", false), ("leftJoin", true)] {
            for join in self.doc.values(node, &rml(property)) {
                self.doc.only(
                    join,
                    &["parentLogicalView", "joinCondition", "field"],
                    "logical view join",
                )?;
                let parent = match self.source(self.doc.required(join, &rml("parentLogicalView"))?)? {
                    Source::View(view) => view,
                    _ => return Err(malformed("rml:parentLogicalView must be a logical view".to_owned())),
                };
                let conditions = self.join_conditions(join)?;
                if conditions.is_empty() {
                    return Err(malformed(format!(
                        "the logical view join {join} has no rml:joinCondition"
                    )));
                }
                joins.push(ViewJoin {
                    left,
                    parent,
                    conditions,
                    fields: self.fields(join, "logical view join")?,
                });
            }
        }
        Ok(View { on, fields, joins })
    }

    /// Expression fields: `(rml:fieldName, rml:reference)`.
    fn fields(&self, node: &Term, role: &str) -> Result<Vec<(String, String)>, SqlCompileError> {
        let mut out = Vec::new();
        for field in self.doc.values(node, &rml("field")) {
            self.doc
                .only(field, &["fieldName", "reference"], &format!("field of a {role}"))?;
            if self.doc.types(field).contains(&rml("IterableField")) {
                return Err(refuse("rml:IterableField", "only expression fields are read"));
            }
            let name = self
                .doc
                .string(field, &rml("fieldName"))?
                .ok_or_else(|| malformed(format!("the field {field} has no rml:fieldName")))?;
            let reference = self
                .doc
                .reference(field, &rml("reference"))?
                .ok_or_else(|| malformed(format!("the field {name} has no rml:reference")))?;
            out.push((name, reference));
        }
        Ok(out)
    }

    /// `(child, parent)` references of every `rml:joinCondition` of `node`.
    fn join_conditions(&self, node: &Term) -> Result<Vec<(String, String)>, SqlCompileError> {
        let mut out = Vec::new();
        for condition in self.doc.values(node, &rml("joinCondition")) {
            self.doc.only(
                condition,
                &["child", "parent", "childMap", "parentMap"],
                "join condition",
            )?;
            let side = |shortcut: &str, map: &str| -> Result<String, SqlCompileError> {
                if let Some(reference) = self.doc.reference(condition, &rml(shortcut))? {
                    return Ok(reference);
                }
                let map_node = self.doc.required(condition, &rml(map))?;
                self.doc.only(map_node, &["reference"], &format!("rml:{map}"))?;
                self.doc
                    .reference(map_node, &rml("reference"))?
                    .ok_or_else(|| refuse(&format!("rml:{map}"), "only a reference-valued map is read"))
            };
            out.push((side("child", "childMap")?, side("parent", "parentMap")?));
        }
        Ok(out)
    }

    fn constant(term: &Term) -> Result<TermRule, SqlCompileError> {
        let (term_type, value) = match term {
            Term::NamedNode(_) => (TermType::Iri, encode(term)?),
            Term::Literal(_) => (TermType::Literal, encode(term)?),
            other => {
                return Err(malformed(format!(
                    "the constant {other} is neither an IRI nor a literal"
                )));
            },
        };
        Ok(TermRule {
            value: Value::Constant(value),
            term_type,
            datatype: None,
            language: None,
            base_iri: None,
        })
    }

    /// A constant- or reference-valued attribute map (datatype or language).
    fn attribute(&self, node: &Term, shortcut: &str, map: &str) -> Result<Option<Value>, SqlCompileError> {
        if let Some(constant) = self.doc.one(node, &rml(shortcut))? {
            return Ok(Some(Value::Constant(encode(constant)?)));
        }
        let Some(map_node) = self.doc.one(node, &rml(map))? else {
            return Ok(None);
        };
        self.doc
            .only(map_node, &["constant", "reference"], &format!("rml:{map}"))?;
        if let Some(constant) = self.doc.one(map_node, &rml("constant"))? {
            return Ok(Some(Value::Constant(encode(constant)?)));
        }
        match self.doc.reference(map_node, &rml("reference"))? {
            Some(reference) => Ok(Some(Value::Reference(reference))),
            None => Err(refuse(
                &format!("rml:{map}"),
                "only constant or reference values are read",
            )),
        }
    }

    fn term_map(
        &self,
        node: &Term,
        position: TermPosition,
        base_iri: Option<&str>,
    ) -> Result<TermRule, SqlCompileError> {
        if let Some(constant) = self.doc.one(node, &rml("constant"))? {
            return Self::constant(constant);
        }
        let reference = self.doc.reference(node, &rml("reference"))?.ok_or_else(|| {
            if !self.doc.values(node, &rml("template")).is_empty() {
                refuse("rml:template", "only rml:reference and rml:constant term maps are read")
            } else {
                malformed(format!("the term map {node} has no rml:reference or rml:constant"))
            }
        })?;
        let datatype = self.attribute(node, "datatype", "datatypeMap")?;
        let language = self.attribute(node, "language", "languageMap")?;
        let term_type = match self.doc.one(node, &rml("termType"))? {
            Some(Term::NamedNode(n)) if n.as_str() == rml("IRI") => TermType::Iri,
            Some(Term::NamedNode(n)) if n.as_str() == rml("BlankNode") => TermType::BlankNode,
            Some(Term::NamedNode(n)) if n.as_str() == rml("Literal") => TermType::Literal,
            Some(Term::NamedNode(n)) => {
                return Err(refuse(
                    &short(n.as_str()),
                    "the term types read are rml:IRI, rml:BlankNode, rml:Literal",
                ));
            },
            Some(other) => return Err(malformed(format!("rml:termType {other} is not an IRI"))),
            // RML-Core §8.4.1: subjects default to IRIs, reference-valued
            // objects to literals.
            None => match position {
                TermPosition::Subject => TermType::Iri,
                TermPosition::Object => TermType::Literal,
            },
        };
        match (position, term_type) {
            (TermPosition::Subject, TermType::Literal) => {
                return Err(malformed(format!("the subject map {node} cannot generate literals")));
            },
            (_, TermType::Iri | TermType::BlankNode) if datatype.is_some() || language.is_some() => {
                return Err(malformed(format!(
                    "the term map {node} declares a datatype or language but does not generate literals"
                )));
            },
            _ => {},
        }
        Ok(TermRule {
            value: Value::Reference(reference),
            term_type,
            datatype,
            language,
            base_iri: base_iri.map(str::to_owned),
        })
    }

    fn rules(&self) -> Result<Vec<Rule>, SqlCompileError> {
        let maps = self.triples_maps();
        if maps.is_empty() {
            return Err(malformed("the RML mapping has no triples map".to_owned()));
        }
        let mut rules = Vec::new();
        for node in &maps {
            let tm = self.triples_map(node)?;
            for class in &tm.classes {
                rules.push(Rule {
                    predicate: RDF_TYPE.to_owned(),
                    source: tm.source.clone(),
                    subject: tm.subject.clone(),
                    object: ObjectRule::Term(Self::constant(&Term::NamedNode(oxrdf::NamedNode::new_unchecked(
                        class.as_str(),
                    )))?),
                });
            }
            for pom in self.doc.values(node, &rml("predicateObjectMap")) {
                self.doc.only(
                    pom,
                    &["predicate", "predicateMap", "object", "objectMap"],
                    "predicate-object map",
                )?;
                let mut predicates = Vec::new();
                for p in self.doc.values(pom, &rml("predicate")) {
                    predicates.push(p.clone());
                }
                for map in self.doc.values(pom, &rml("predicateMap")) {
                    self.doc.only(map, &["constant"], "predicate map")?;
                    let constant = self
                        .doc
                        .one(map, &rml("constant"))?
                        .ok_or_else(|| refuse("rml:predicateMap", "only constant predicate maps are read"))?;
                    predicates.push(constant.clone());
                }
                let mut objects = Vec::new();
                for object in self.doc.values(pom, &rml("object")) {
                    objects.push(ObjectRule::Term(Self::constant(object)?));
                }
                for map in self.doc.values(pom, &rml("objectMap")) {
                    objects.push(self.object_map(map, &tm, tm.base_iri.as_deref())?);
                }
                if predicates.is_empty() || objects.is_empty() {
                    return Err(malformed(format!(
                        "the predicate-object map {pom} needs a predicate and an object"
                    )));
                }
                for predicate in &predicates {
                    let Term::NamedNode(predicate) = predicate else {
                        return Err(malformed(format!("the predicate {predicate} is not an IRI")));
                    };
                    for object in &objects {
                        rules.push(Rule {
                            predicate: predicate.as_str().to_owned(),
                            source: tm.source.clone(),
                            subject: tm.subject.clone(),
                            object: object.clone(),
                        });
                    }
                }
            }
        }
        Ok(rules)
    }

    fn object_map(
        &self,
        map: &Term,
        child: &TriplesMap,
        base_iri: Option<&str>,
    ) -> Result<ObjectRule, SqlCompileError> {
        let Some(parent) = self.doc.one(map, &rml("parentTriplesMap"))? else {
            self.doc.only(
                map,
                &[
                    "reference",
                    "constant",
                    "termType",
                    "datatype",
                    "datatypeMap",
                    "language",
                    "languageMap",
                ],
                "object map",
            )?;
            return Ok(ObjectRule::Term(self.term_map(map, TermPosition::Object, base_iri)?));
        };
        self.doc
            .only(map, &["parentTriplesMap", "joinCondition"], "referencing object map")?;
        let parent = self.triples_map(parent)?;
        let conditions = self.join_conditions(map)?;
        // RML-Core §9.2: without join conditions the two sources must be the same.
        if conditions.is_empty() && parent.source != child.source {
            return Err(malformed(format!(
                "the referencing object map {map} joins different logical sources without rml:joinCondition"
            )));
        }
        Ok(ObjectRule::Parent {
            source: parent.source,
            subject: parent.subject,
            conditions,
        })
    }
}

#[derive(Clone, Copy)]
enum TermPosition {
    Subject,
    Object,
}

impl<D: Dialect> Tables<D> {
    /// Reads an RML mapping (Turtle); see the [module docs](self) for the
    /// subset read. Unqualified table names are qualified with `schema` when
    /// one is given (`schema.table`).
    pub(crate) fn from_rml(turtle: &str, schema: Option<&str>, dialect: D) -> Result<Self, SqlCompileError> {
        let doc = Doc::parse(turtle)?;
        let reader = Reader {
            doc: &doc,
            sources: RefCell::new(BTreeSet::new()),
        };
        let rules = reader.rules()?;
        // RML-IO §2 puts the location on the rml:Source. The host gives one
        // location, `schema`, so one source is what the mapping may name.
        let sources = reader.sources.into_inner();
        if sources.len() > 1 {
            return Err(refuse(
                "rml:source",
                &format!(
                    "the mapping names {} distinct rml:Source nodes; one is read, located by the schema the host gives",
                    sources.len()
                ),
            ));
        }
        let schema = schema
            .map(parse_object_name)
            .transpose()
            .map_err(|e| malformed(format!("the schema: {e}")))?;
        Ok(Tables::new(rules, schema, dialect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::sql::DuckDb;

    const PREFIXES: &str = "@prefix rml: <http://w3id.org/rml/> . @prefix ex: <http://example.org/> .\n";

    fn read(body: &str) -> Result<Tables<DuckDb>, SqlCompileError> {
        Tables::from_rml(&format!("{PREFIXES}{body}"), None, DuckDb)
    }

    const SOURCE: &str =
        r#"rml:logicalSource [ rml:source [ ] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator "person" ]"#;

    #[test]
    fn a_table_with_a_class_and_a_column_reads() {
        let tables = read(&format!(
            r#"<#P> a rml:TriplesMap ; {SOURCE} ;
                 rml:subjectMap [ rml:reference "iri" ; rml:class ex:Person ] ;
                 rml:predicateObjectMap [ rml:predicate ex:name ; rml:objectMap [ rml:reference "name" ] ] ."#
        ))
        .unwrap();
        assert_eq!(
            tables.predicates(),
            [
                "http://example.org/name",
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
            ]
        );
    }

    #[test]
    fn rml_terms_outside_the_subset_are_refused_by_name() {
        let cases = [
            (
                format!(r#"<#P> {SOURCE} ; rml:subjectMap [ rml:template "http://e/{{id}}" ] ."#),
                "rml:template",
            ),
            (
                format!(r#"<#P> {SOURCE} ; rml:subjectMap [ rml:reference "iri" ; rml:graphMap [ rml:constant ex:g ] ] ."#),
                "rml:graphMap",
            ),
            (
                r#"<#P> rml:logicalSource [ rml:source [ ] ; rml:referenceFormulation rml:SQL2008Query ; rml:iterator "SELECT 1" ] ; rml:subjectMap [ rml:reference "iri" ] ."#
                    .to_owned(),
                "rml:SQL2008Query",
            ),
            (
                format!(r#"<#P> {SOURCE} ; rml:subjectMap [ rml:reference "iri" ; rml:termType rml:UnsafeIRI ] ."#),
                "rml:UnsafeIRI",
            ),
        ];
        for (body, term) in cases {
            match read(&body) {
                Err(SqlCompileError::UnsupportedRml(message)) => {
                    assert!(message.starts_with(term), "{message} names {term}")
                },
                other => panic!("{term} must be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_reference_without_join_condition_needs_the_same_source() {
        let other =
            r#"rml:logicalSource [ rml:source [ ] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator "pet" ]"#;
        let refused = read(&format!(
            r#"<#P> {SOURCE} ; rml:subjectMap [ rml:reference "iri" ] ;
                 rml:predicateObjectMap [ rml:predicate ex:pet ; rml:objectMap [ rml:parentTriplesMap <#Pet> ] ] .
               <#Pet> {other} ; rml:subjectMap [ rml:reference "iri" ] ."#
        ));
        assert!(matches!(refused, Err(SqlCompileError::Mapping(_))), "{refused:?}");
    }

    #[test]
    fn references_are_sql_identifiers_delimited_or_not() {
        let tables = read(&format!(
            r#"<#P> {SOURCE} ; rml:subjectMap [ rml:reference "\"Iri\"" ] ;
                 rml:predicateObjectMap [ rml:predicate ex:born ; rml:objectMap [ rml:reference "\"birth_year\"" ] ] ."#
        ));
        assert!(tables.is_ok(), "{tables:?}");
        let bad = read(&format!(r#"<#P> {SOURCE} ; rml:subjectMap [ rml:reference "a b" ] ."#));
        assert!(matches!(bad, Err(SqlCompileError::Mapping(_))), "{bad:?}");
    }

    #[test]
    fn a_mapping_names_one_source() {
        let other = r#"rml:logicalSource [ rml:source <#elsewhere> ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator "pet" ]"#;
        let refused = read(&format!(
            r#"<#elsewhere> a rml:Source ; <http://example.org/at> "somewhere" .
               <#P> {SOURCE} ; rml:subjectMap [ rml:reference "iri" ] .
               <#Pet> {other} ; rml:subjectMap [ rml:reference "iri" ] ."#
        ));
        match refused {
            Err(SqlCompileError::UnsupportedRml(message)) => assert!(message.starts_with("rml:source"), "{message}"),
            other => panic!("two sources must be refused, got {other:?}"),
        }
    }
}
