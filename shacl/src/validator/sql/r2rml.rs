//! Reads an R2RML mapping (W3C Recommendation, namespace
//! `http://www.w3.org/ns/r2rml#`) into [`Tables`].
//!
//! What is read is R2RML minus what a single data graph cannot hold:
//!
//! - **Logical tables** (§5): `rr:tableName`, or an R2RML view, `rr:sqlQuery`
//!   with an optional `rr:sqlVersion rr:SQL2008`, read as a derived table.
//!   The query's output columns must be uniquely named (§5.2).
//! - **Term maps** (§7): `rr:column`, `rr:template` or `rr:constant` (and the
//!   `rr:subject`, `rr:predicate`, `rr:object` shortcuts), with `rr:termType`,
//!   `rr:datatype`, `rr:language`, and `rr:class` on subject maps.
//!   `rr:inverseExpression` is a hint for query rewriting that never changes
//!   the triples (§7.6), and is accepted and not read.
//! - **Referencing object maps** (§8): `rr:parentTriplesMap` with
//!   `rr:joinCondition` (`rr:child`, `rr:parent`), or none when both triples
//!   maps read the same logical table.
//!
//! Refused by name, with [`SqlCompileError::UnsupportedR2rml`], never ignored:
//! graph maps (`rr:graph`, `rr:graphMap`), since SHACL validates one data
//! graph and named graphs would have to be merged or chosen; predicate maps
//! that are not constant, since the engine reads a mapping one predicate at a
//! time; and an `rr:sqlVersion` other than `rr:SQL2008`. Terms outside the
//! R2RML namespace (labels, comments) have no effect on the mapping and are
//! allowed.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{parse_identifier, parse_object_name};
use crate::validator::sql::dialect::Dialect;
use crate::validator::sql::tables::{ObjectRule, Part, Rule, Source, Tables, TermRule, TermType, Value};
use crate::validator::sql::term::encode;
use oxrdf::{NamedOrBlankNode, Term};
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::term::Triple;
use rudof_rdf::{NeighsRDF, RDFFormat};
use sqlparser::ast::{Expr, ObjectName, Query, SelectItem, SetExpr, Statement, visit_relations_mut};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

const RR: &str = "http://www.w3.org/ns/r2rml#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
/// The base a mapping document's own relative IRIs (`<#TriplesMap>`) resolve
/// against; it names mapping nodes only and never reaches a generated term.
const DOCUMENT_BASE: &str = "file:///mapping.r2rml.ttl";

fn rr(local: &str) -> String {
    format!("{RR}{local}")
}

fn refuse(term: &str, why: &str) -> SqlCompileError {
    SqlCompileError::UnsupportedR2rml(format!("{term}: {why}"))
}

fn malformed(message: String) -> SqlCompileError {
    SqlCompileError::Mapping(message)
}

fn short(iri: &str) -> String {
    match iri.strip_prefix(RR) {
        Some(local) => format!("rr:{local}"),
        None => format!("<{iri}>"),
    }
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

impl Doc {
    fn parse(turtle: &str) -> Result<Self, SqlCompileError> {
        let graph = OxigraphInMemory::from_str(turtle, &RDFFormat::Turtle, Some(DOCUMENT_BASE), &ReaderMode::Strict)
            .map_err(|e| malformed(format!("the R2RML mapping does not parse as Turtle: {e}")))?;
        let mut arcs: BTreeMap<String, Vec<(String, Term)>> = BTreeMap::new();
        let mut nodes = BTreeMap::new();
        let triples = graph
            .triples()
            .map_err(|e| malformed(format!("reading the R2RML mapping: {e}")))?;
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
        match self.values(node, predicate).as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one)),
            _ => Err(malformed(format!("{node} has more than one {}", short(predicate)))),
        }
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

    fn iri(&self, node: &Term, predicate: &str) -> Result<Option<String>, SqlCompileError> {
        match self.one(node, predicate)? {
            None => Ok(None),
            Some(Term::NamedNode(n)) => Ok(Some(n.as_str().to_owned())),
            Some(other) => Err(malformed(format!("{} {other} is not an IRI", short(predicate)))),
        }
    }

    /// A column name: a SQL identifier, delimited or not (§6), by its value.
    fn column(&self, node: &Term, predicate: &str) -> Result<Option<String>, SqlCompileError> {
        self.string(node, predicate)?
            .map(|text| column(&text).map_err(|e| malformed(format!("{} of {node}: {e}", short(predicate)))))
            .transpose()
    }

    /// Refuses every R2RML property of `node` outside `allowed`.
    fn only(&self, node: &Term, allowed: &[&str], role: &str) -> Result<(), SqlCompileError> {
        for (predicate, _) in self.arcs.get(&key(node)).into_iter().flatten() {
            if let Some(local) = predicate.strip_prefix(RR)
                && !allowed.contains(&local)
            {
                return Err(match local {
                    "graph" | "graphMap" => refuse(
                        &format!("rr:{local}"),
                        "SHACL validates one data graph; named graphs are not read",
                    ),
                    _ => malformed(format!("rr:{local} is not allowed on a {role}")),
                });
            }
        }
        Ok(())
    }
}

fn column(text: &str) -> Result<String, String> {
    parse_identifier(text).map(|ident| ident.value)
}

/// The parts of an `rr:template` (§7.3): `{column}` between literal text, with
/// `\{`, `\}` and `\\` escaping a brace or a backslash.
fn parse_template(text: &str) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(escaped @ ('{' | '}' | '\\')) => literal.push(escaped),
                _ => return Err(format!("the template '{text}' has a dangling backslash")),
            },
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped) => name.push(escaped),
                            None => return Err(format!("the template '{text}' has a dangling backslash")),
                        },
                        Some(c) => name.push(c),
                        None => return Err(format!("the template '{text}' has an unclosed '{{'")),
                    }
                }
                if !literal.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut literal)));
                }
                parts.push(Part::Column(column(&name)?));
            },
            '}' => return Err(format!("the template '{text}' has an unopened '}}'")),
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        parts.push(Part::Text(literal));
    }
    Ok(parts)
}

/// Where a term map sits: what it may generate, and what it defaults to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Subject,
    Object,
}

/// What a triples map reads, and how it names its subject.
struct TriplesMap {
    source: Source,
    subject: TermRule,
    classes: Vec<String>,
}

/// Reads R2RML into the [`Tables`] model.
struct Reader<'d> {
    doc: &'d Doc,
    /// What an unqualified table name in an R2RML view is qualified with.
    schema: Option<&'d ObjectName>,
}

impl Reader<'_> {
    /// Every triples map: a node typed `rr:TriplesMap`, or one with a
    /// logical table (§6 lets the type be inferred).
    fn triples_maps(&self) -> Vec<Term> {
        self.doc
            .arcs
            .iter()
            .filter(|(_, arcs)| {
                arcs.iter().any(|(p, o)| {
                    (p == RDF_TYPE && matches!(o, Term::NamedNode(n) if n.as_str() == rr("TriplesMap")))
                        || *p == rr("logicalTable")
                })
            })
            .filter_map(|(node, _)| self.doc.nodes.get(node).cloned())
            .collect()
    }

    fn triples_map(&self, node: &Term) -> Result<TriplesMap, SqlCompileError> {
        self.doc.only(
            node,
            &["logicalTable", "subjectMap", "subject", "predicateObjectMap"],
            "triples map",
        )?;
        let table = self
            .doc
            .one(node, &rr("logicalTable"))?
            .ok_or_else(|| malformed(format!("the triples map {node} has no rr:logicalTable")))?;
        let source = self.logical_table(table)?;
        let maps = self.doc.values(node, &rr("subjectMap"));
        let constants = self.doc.values(node, &rr("subject"));
        let (subject, classes) = match (maps.as_slice(), constants.as_slice()) {
            ([map], []) => {
                self.doc.only(
                    map,
                    &[
                        "column",
                        "template",
                        "constant",
                        "termType",
                        "class",
                        "inverseExpression",
                    ],
                    "subject map",
                )?;
                let mut classes = Vec::new();
                for class in self.doc.values(map, &rr("class")) {
                    match class {
                        Term::NamedNode(n) => classes.push(n.as_str().to_owned()),
                        other => return Err(malformed(format!("rr:class {other} is not an IRI"))),
                    }
                }
                (self.term_map(map, Position::Subject)?, classes)
            },
            ([], [constant]) => (Self::constant(constant)?, Vec::new()),
            _ => {
                return Err(malformed(format!(
                    "the triples map {node} must have exactly one subject map"
                )));
            },
        };
        Ok(TriplesMap {
            source,
            subject,
            classes,
        })
    }

    fn logical_table(&self, node: &Term) -> Result<Source, SqlCompileError> {
        self.doc
            .only(node, &["tableName", "sqlQuery", "sqlVersion"], "logical table")?;
        match (
            self.doc.string(node, &rr("tableName"))?,
            self.doc.string(node, &rr("sqlQuery"))?,
        ) {
            (Some(name), None) => parse_object_name(&name)
                .map(Source::Table)
                .map_err(|e| malformed(format!("rr:tableName {name}: {e}"))),
            (None, Some(sql)) => {
                for version in self.doc.values(node, &rr("sqlVersion")) {
                    if !matches!(version, Term::NamedNode(n) if n.as_str() == rr("SQL2008")) {
                        return Err(refuse(
                            &version.to_string(),
                            "the one SQL version read is rr:SQL2008, Core SQL 2008",
                        ));
                    }
                }
                self.view(&sql).map(|query| Source::Query(Box::new(query)))
            },
            _ => Err(malformed(format!(
                "the logical table {node} must have exactly one of rr:tableName and rr:sqlQuery"
            ))),
        }
    }

    /// An R2RML view (§5.2): one query, its output columns uniquely named,
    /// its unqualified table names qualified with the schema.
    fn view(&self, sql: &str) -> Result<Query, SqlCompileError> {
        let invalid = |why: String| malformed(format!("the rr:sqlQuery is not a valid SQL query: {why}"));
        let mut statements = Parser::parse_sql(&GenericDialect {}, sql).map_err(|e| invalid(e.to_string()))?;
        let mut query = match (statements.pop(), statements.is_empty()) {
            (Some(Statement::Query(query)), true) => *query,
            _ => return Err(invalid("it is not one SELECT statement".to_owned())),
        };
        if let SetExpr::Select(select) = query.body.as_ref() {
            let mut names = BTreeSet::new();
            for item in &select.projection {
                let name = match item {
                    SelectItem::ExprWithAlias { alias, .. } => Some(&alias.value),
                    SelectItem::UnnamedExpr(Expr::Identifier(ident)) => Some(&ident.value),
                    SelectItem::UnnamedExpr(Expr::CompoundIdentifier(idents)) => idents.last().map(|i| &i.value),
                    _ => None,
                };
                if let Some(name) = name
                    && !names.insert(name.to_lowercase())
                {
                    return Err(malformed(format!(
                        "the rr:sqlQuery names the column {name} twice; an R2RML view's columns are unique"
                    )));
                }
            }
        }
        if let Some(schema) = self.schema {
            let ctes: BTreeSet<String> = query
                .with
                .iter()
                .flat_map(|with| &with.cte_tables)
                .map(|cte| cte.alias.name.value.clone())
                .collect();
            let _ = visit_relations_mut(&mut query, |name| {
                if name.0.len() == 1 && !name.0[0].as_ident().is_some_and(|i| ctes.contains(&i.value)) {
                    name.0 = schema.0.iter().chain(name.0.iter()).cloned().collect();
                }
                ControlFlow::<()>::Continue(())
            });
        }
        Ok(query)
    }

    fn constant(term: &Term) -> Result<TermRule, SqlCompileError> {
        let term_type = match term {
            Term::NamedNode(_) => TermType::Iri,
            Term::Literal(_) => TermType::Literal,
            other => {
                return Err(malformed(format!(
                    "the constant {other} is neither an IRI nor a literal"
                )));
            },
        };
        Ok(TermRule {
            value: Value::Constant(encode(term)?),
            term_type,
            datatype: None,
            language: None,
        })
    }

    /// A column-, template- or constant-valued term map (§7).
    fn term_map(&self, node: &Term, position: Position) -> Result<TermRule, SqlCompileError> {
        let column = self.doc.column(node, &rr("column"))?;
        let template = self.doc.string(node, &rr("template"))?;
        let constant = self.doc.one(node, &rr("constant"))?;
        let value = match (column, template, constant) {
            (Some(column), None, None) => Value::Column(column),
            (None, Some(text), None) => {
                Value::Template(parse_template(&text).map_err(|e| malformed(format!("rr:template of {node}: {e}")))?)
            },
            (None, None, Some(constant)) => return Self::constant(constant),
            _ => {
                return Err(malformed(format!(
                    "the term map {node} must have exactly one of rr:column, rr:template and rr:constant"
                )));
            },
        };
        let datatype = self.doc.iri(node, &rr("datatype"))?;
        let language = self.doc.string(node, &rr("language"))?;
        if let Some(tag) = &language
            && oxrdf::Literal::new_language_tagged_literal("", tag).is_err()
        {
            return Err(malformed(format!("rr:language \"{tag}\" is not a language tag")));
        }
        // §7.4: a column-valued object map, or one with a datatype or a
        // language, generates literals; everything else generates IRIs.
        let literal_by_default = position == Position::Object
            && (matches!(value, Value::Column(_)) || datatype.is_some() || language.is_some());
        let term_type = match self.doc.iri(node, &rr("termType"))? {
            Some(t) if t == rr("IRI") => TermType::Iri,
            Some(t) if t == rr("BlankNode") => TermType::BlankNode,
            Some(t) if t == rr("Literal") => TermType::Literal,
            Some(other) => return Err(malformed(format!("rr:termType {} is not a term type", short(&other)))),
            None if literal_by_default => TermType::Literal,
            None => TermType::Iri,
        };
        if position == Position::Subject && term_type == TermType::Literal {
            return Err(malformed(format!("the subject map {node} cannot generate literals")));
        }
        if term_type != TermType::Literal && (datatype.is_some() || language.is_some()) {
            return Err(malformed(format!(
                "the term map {node} declares a datatype or language but does not generate literals"
            )));
        }
        if datatype.is_some() && language.is_some() {
            return Err(malformed(format!(
                "the term map {node} declares both a datatype and a language"
            )));
        }
        Ok(TermRule {
            value,
            term_type,
            datatype,
            language,
        })
    }

    fn rules(&self) -> Result<Vec<Rule>, SqlCompileError> {
        let maps = self.triples_maps();
        if maps.is_empty() {
            return Err(malformed("the R2RML mapping has no triples map".to_owned()));
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
            for pom in self.doc.values(node, &rr("predicateObjectMap")) {
                self.doc.only(
                    pom,
                    &["predicate", "predicateMap", "object", "objectMap"],
                    "predicate-object map",
                )?;
                let mut predicates: Vec<Term> = self.doc.values(pom, &rr("predicate")).into_iter().cloned().collect();
                for map in self.doc.values(pom, &rr("predicateMap")) {
                    if !self.doc.values(map, &rr("column")).is_empty()
                        || !self.doc.values(map, &rr("template")).is_empty()
                    {
                        return Err(refuse(
                            "rr:predicateMap",
                            "only constant predicate maps are read: the engine reads a mapping a predicate at a time",
                        ));
                    }
                    self.doc.only(map, &["constant"], "predicate map")?;
                    let constant = self
                        .doc
                        .one(map, &rr("constant"))?
                        .ok_or_else(|| malformed(format!("the predicate map {map} has no rr:constant")))?;
                    predicates.push(constant.clone());
                }
                let mut objects = Vec::new();
                for object in self.doc.values(pom, &rr("object")) {
                    objects.push(ObjectRule::Term(Self::constant(object)?));
                }
                for map in self.doc.values(pom, &rr("objectMap")) {
                    objects.push(self.object_map(map, &tm)?);
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

    fn object_map(&self, map: &Term, child: &TriplesMap) -> Result<ObjectRule, SqlCompileError> {
        let Some(parent) = self.doc.one(map, &rr("parentTriplesMap"))? else {
            self.doc.only(
                map,
                &[
                    "column",
                    "template",
                    "constant",
                    "termType",
                    "datatype",
                    "language",
                    "inverseExpression",
                ],
                "object map",
            )?;
            return Ok(ObjectRule::Term(self.term_map(map, Position::Object)?));
        };
        self.doc
            .only(map, &["parentTriplesMap", "joinCondition"], "referencing object map")?;
        let parent = self.triples_map(parent)?;
        let mut conditions = Vec::new();
        for condition in self.doc.values(map, &rr("joinCondition")) {
            self.doc.only(condition, &["child", "parent"], "join condition")?;
            let side = |local: &str| {
                self.doc
                    .column(condition, &rr(local))?
                    .ok_or_else(|| malformed(format!("the join condition {condition} has no rr:{local}")))
            };
            conditions.push((side("child")?, side("parent")?));
        }
        // §8: without join conditions the two logical tables must be the same.
        if conditions.is_empty() && parent.source != child.source {
            return Err(malformed(format!(
                "the referencing object map {map} joins different logical tables without rr:joinCondition"
            )));
        }
        Ok(ObjectRule::Parent {
            source: parent.source,
            subject: parent.subject,
            conditions,
        })
    }
}

impl<D: Dialect> Tables<D> {
    /// Reads an R2RML mapping (Turtle); see the [module docs](self) for what
    /// is read. Unqualified table names are qualified with `schema` when one
    /// is given (`schema.table`); relative IRIs are resolved against
    /// `base_iri` (R2RML §11).
    pub(crate) fn from_r2rml(
        turtle: &str,
        schema: Option<&str>,
        base_iri: Option<&str>,
        dialect: D,
    ) -> Result<Self, SqlCompileError> {
        let schema = schema
            .map(parse_object_name)
            .transpose()
            .map_err(|e| malformed(format!("the schema: {e}")))?;
        let doc = Doc::parse(turtle)?;
        let rules = Reader {
            doc: &doc,
            schema: schema.as_ref(),
        }
        .rules()?;
        Ok(Tables::new(rules, schema, base_iri.map(str::to_owned), dialect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::sql::dialect::DuckDb;

    const PREFIXES: &str = "@prefix rr: <http://www.w3.org/ns/r2rml#> . @prefix ex: <http://example.org/> .\n";
    const TABLE: &str = r#"rr:logicalTable [ rr:tableName "person" ]"#;

    fn read(body: &str) -> Result<Tables<DuckDb>, SqlCompileError> {
        Tables::from_r2rml(&format!("{PREFIXES}{body}"), None, None, DuckDb)
    }

    #[test]
    fn a_table_with_a_class_and_a_column_reads() {
        let tables = read(&format!(
            r#"<#P> a rr:TriplesMap ; {TABLE} ;
                 rr:subjectMap [ rr:column "iri" ; rr:class ex:Person ] ;
                 rr:predicateObjectMap [ rr:predicate ex:name ; rr:objectMap [ rr:column "name" ] ] ."#
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
    fn graph_maps_and_computed_predicates_are_refused_by_name() {
        let cases = [
            (
                format!(r#"<#P> {TABLE} ; rr:subjectMap [ rr:column "iri" ; rr:graphMap [ rr:constant ex:g ] ] ."#),
                "rr:graphMap",
            ),
            (
                format!(
                    r#"<#P> {TABLE} ; rr:subjectMap [ rr:column "iri" ] ;
                         rr:predicateObjectMap [ rr:predicateMap [ rr:column "p" ] ; rr:object ex:o ] ."#
                ),
                "rr:predicateMap",
            ),
            (
                r#"<#P> rr:logicalTable [ rr:sqlQuery "SELECT 1 AS iri" ; rr:sqlVersion <http://example.org/Oracle> ] ;
                     rr:subjectMap [ rr:column "iri" ] ."#
                    .to_owned(),
                "<http://example.org/Oracle>",
            ),
        ];
        for (body, term) in cases {
            match read(&body) {
                Err(SqlCompileError::UnsupportedR2rml(message)) => {
                    assert!(message.starts_with(term), "{message} names {term}")
                },
                other => panic!("{term} must be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn templates_split_into_text_and_columns() {
        assert_eq!(
            parse_template(r#"http://e/{"ID"}/\{x\}{name}"#).unwrap(),
            [
                Part::Text("http://e/".into()),
                Part::Column("ID".into()),
                Part::Text("/{x}".into()),
                Part::Column("name".into()),
            ]
        );
        assert!(parse_template("http://e/{id").is_err());
        assert!(parse_template("http://e/}").is_err());
    }

    #[test]
    fn a_view_names_each_column_once() {
        let view = |sql: &str| {
            read(&format!(
                r#"<#P> rr:logicalTable [ rr:sqlQuery "{sql}" ] ; rr:subjectMap [ rr:column "iri" ] ."#
            ))
        };
        assert!(view("SELECT a.iri, b.x FROM a JOIN b ON a.k = b.k").is_ok());
        assert!(matches!(
            view("SELECT a.iri, b.iri FROM a, b"),
            Err(SqlCompileError::Mapping(_))
        ));
        assert!(matches!(view("SELEKT 1"), Err(SqlCompileError::Mapping(_))));
    }

    #[test]
    fn a_reference_without_join_condition_needs_the_same_logical_table() {
        let refused = read(&format!(
            r#"<#P> {TABLE} ; rr:subjectMap [ rr:column "iri" ] ;
                 rr:predicateObjectMap [ rr:predicate ex:pet ; rr:objectMap [ rr:parentTriplesMap <#Pet> ] ] .
               <#Pet> rr:logicalTable [ rr:tableName "pet" ] ; rr:subjectMap [ rr:column "iri" ] ."#
        ));
        assert!(matches!(refused, Err(SqlCompileError::Mapping(_))), "{refused:?}");
    }

    #[test]
    fn a_subject_map_cannot_generate_literals() {
        let refused = read(&format!(
            r#"<#P> {TABLE} ; rr:subjectMap [ rr:column "iri" ; rr:termType rr:Literal ] ."#
        ));
        assert!(matches!(refused, Err(SqlCompileError::Mapping(_))), "{refused:?}");
    }
}
