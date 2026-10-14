//! The mapping as a query: the triples it generates, in the seven columns
//! `s_type, s_value, p, o_type, o_value, o_datatype, o_lang` that
//! `shacl::validator::sql` reads (a test in `shacl` holds [`COLUMNS`] to its own).
//!
//! ```text
//! triples(&Mapping, base: Option<&str>, tables: impl Fn(&str) -> ObjectName) -> Result<Query, Error>
//! ```
//!
//! The query is a `UNION ALL` with a branch per class of a subject map, per
//! predicate-object pair and per referencing object map, each over the logical
//! source of its triples map. A host runs it as it likes, usually as
//! `CREATE OR REPLACE VIEW … AS`. The dialect is DuckDB's.
//!
//! **The logical sources.** `rml:SQL2008Table` is `SELECT * FROM {table}`
//! (RML-IO §3.1), the table being `tables(name)`: the host maps the name the
//! mapping uses to the relation it holds. A `rml:SQL2008Query` is parsed, and the
//! name of each table in its `FROM` (a single identifier, and not a table
//! function, a CTE it declares, or a name with a schema) goes through `tables`
//! too.
//!
//! **Which RML-Core semantics are exact.** One row is one logical iteration, and
//! the algorithm of §12.1 is followed: a branch per `<predicate, object>`, a
//! referencing object map as a join of the child and the parent iterations over
//! their join conditions (§9.2) or, with none, over one iteration (the sources
//! being equal). The output of a mapping is a set (§12.1.1: "RDF graphs MUST NOT
//! contain duplicate triples"); this relation may hold a triple twice, as
//! `shacl`'s documents, and the reader reads it as a set.
//!
//! **Which are not.** A query cannot abort, so the *data errors* of §4.3 — an
//! `rml:IRI` value that is not a valid IRI, an ill-typed datatype override, a
//! language tag that is not BCP 47 — are not raised: only the shape of an
//! absolute IRI (a scheme) is looked at, a datatype override is trusted, and a
//! language tag that is NULL or empty generates no term. A `rml:IRI` value is not
//! otherwise validated. Natural datatypes cover the types of
//! [`term::natural_datatype`](term); a column of a nested type (a struct, a list)
//! cannot be a reference, and the engine says so when it plans the query.

mod build;
mod term;

use crate::Error;
use crate::ast::{Iterable, Mapping, ObjectMap, Position, TriplesMap};
use crate::vocab::RDF;
use build::{col, derived, eq, item, join, none, null_varchar, query, select, string, table, union_all};
use sqlparser::ast::{Expr, ObjectName, Query, SelectItem, TableFactor, Visit, VisitMut, Visitor, VisitorMut};
use sqlparser::dialect::DuckDbDialect;
use sqlparser::parser::Parser;
use std::collections::HashSet;
use std::ops::ControlFlow;
use term::{Term, column_name};

/// The base IRI when neither the triples map nor the environment gives one (RML-Core §4.1.1).
pub const DEFAULT_BASE: &str = "http://example.org/";

/// The columns of the relation, in order. A term is spread over a type, a
/// value, a datatype and a language (SPARQL 1.1 Query Results JSON, §3.2.2, names
/// a term's members); a subject has no datatype or language, and the predicate
/// is `p`.
pub const COLUMNS: [&str; 7] = ["s_type", "s_value", "p", "o_type", "o_value", "o_datatype", "o_lang"];

/// The triples of `mapping` over the tables `tables` resolves.
///
/// `base` is the base IRI of the execution environment (RML-Core §4.1.1: "The base
/// IRI MUST be defined within the mapping document for each Triples Map or as
/// execution environment"): a triples map's own `rml:baseIRI` wins, and with
/// neither it is [`DEFAULT_BASE`], which the specification fixes ("the default base
/// IRI to be used by the RML Processor MUST be http://example.org/"). A generated
/// IRI that is not absolute is prepended with it (§12.2).
pub fn triples(mapping: &Mapping, base: Option<&str>, tables: impl Fn(&str) -> ObjectName) -> Result<Query, Error> {
    mapping.validate()?;
    let mut branches = Vec::new();
    for m in &mapping.triples_maps {
        branches.extend(triples_map(mapping, m, base, &tables)?);
    }
    Ok(query(match union_all(branches) {
        Some(body) => body,
        None => none(COLUMNS.iter().map(|c| item(null_varchar(), c)).collect()),
    }))
}

/// A SQL object name as a host writes it: `"job".triples`, `Person`, `"My Table"`. The name is read by the SQL
/// grammar and is never split on a dot by hand, so a delimited identifier keeps its case and its dots.
pub fn object_name(text: &str) -> Result<ObjectName, Error> {
    let table = |reason: String| Error::Table {
        name: text.to_owned(),
        reason,
    };
    let mut parser = Parser::new(&DuckDbDialect {})
        .try_with_sql(text)
        .map_err(|e| table(e.to_string()))?;
    let name = parser.parse_object_name(false).map_err(|e| table(e.to_string()))?;
    parser
        .expect_token(&sqlparser::tokenizer::Token::EOF)
        .map_err(|_| table("not a SQL object name".to_owned()))?;
    Ok(name)
}

/// The statement that creates, or replaces, the view `into` as the relation of `mapping`. The host runs it.
///
/// `tables` resolves a table the mapping names to the SQL name of the relation that holds it (`"job"."Person"`);
/// a name it cannot resolve is its `Err`, and fails the call. `into` and every resolved name are read as SQL object
/// names ([`object_name`]), so no text of the host's reaches the statement unparsed.
pub fn create_view(
    mapping: &Mapping,
    base: Option<&str>,
    into: &str,
    tables: impl Fn(&str) -> Result<String, String>,
) -> Result<String, Error> {
    let into = object_name(into)?;
    let failure = std::cell::RefCell::new(None);
    let query = triples(mapping, base, |name| {
        match tables(name).and_then(|resolved| object_name(&resolved).map_err(|e| e.to_string())) {
            Ok(resolved) => resolved,
            Err(reason) => {
                failure.borrow_mut().get_or_insert(Error::Table {
                    name: name.to_owned(),
                    reason,
                });
                ObjectName::from(vec![sqlparser::ast::Ident::new(name)])
            },
        }
    })?;
    match failure.into_inner() {
        Some(e) => Err(e),
        None => Ok(format!("CREATE OR REPLACE VIEW {into} AS {query}")),
    }
}

/// A branch of the relation: the subject, the predicate and the object terms.
fn row(subject: &Term, predicate: &Expr, object: &Term) -> Vec<SelectItem> {
    let values = [
        &subject.kind,
        &subject.value,
        predicate,
        &object.kind,
        &object.value,
        &object.datatype,
        &object.lang,
    ];
    COLUMNS.iter().zip(values).map(|(c, e)| item(e.clone(), c)).collect()
}

/// The base IRI of a triples map: its own, the environment's, or the default (§4.1.1).
fn base_of<'a>(m: &'a TriplesMap, environment: Option<&'a str>) -> &'a str {
    m.base_iri
        .as_ref()
        .map(|b| b.as_str())
        .or(environment)
        .unwrap_or(DEFAULT_BASE)
}

fn triples_map(
    mapping: &Mapping,
    m: &TriplesMap,
    environment: Option<&str>,
    tables: &impl Fn(&str) -> ObjectName,
) -> Result<Vec<sqlparser::ast::SetExpr>, Error> {
    let at = m.id.to_string();
    let (child, parent) = ("t", "o");
    let base = base_of(m, environment);
    let subject = term::term(&m.subject_map.term, Position::Subject, child, base);
    let from = || relation(&m.logical_source.iterable, child, &at, tables);
    let mut branches = Vec::new();

    // §6.2: "for each RDF term generated by the subject map, RDF triples with
    // predicate rdf:type and the class IRI as object".
    for class in &m.subject_map.classes {
        let object = term::term(&crate::TermMap::iri(class.clone()), Position::Object, child, base);
        branches.push(select(
            row(&subject, &string(&format!("{RDF}type")), &object),
            from()?,
            Vec::new(),
            subject.required.clone(),
        ));
    }

    for pom in &m.predicate_object_maps {
        for predicate in &pom.predicates {
            let predicate = term::term(predicate, Position::Predicate, child, base);
            for object in &pom.objects {
                let mut conditions = subject.required.clone();
                conditions.extend(predicate.required.clone());
                match object {
                    ObjectMap::Term(map) => {
                        let object = term::term(map, Position::Object, child, base);
                        conditions.extend(object.required.clone());
                        branches.push(select(
                            row(&subject, &predicate.value, &object),
                            from()?,
                            Vec::new(),
                            conditions,
                        ));
                    },
                    ObjectMap::Reference(r) => {
                        let parent_map = mapping
                            .triples_map(&r.parent)
                            .ok_or_else(|| Error::invalid(&at, "the parent triples map is not in the mapping"))?;
                        if r.join_conditions.is_empty() {
                            // §9.2: the logical sources are effectively equal: one iteration serves both.
                            let object = term::term(
                                &parent_map.subject_map.term,
                                Position::Subject,
                                child,
                                base_of(parent_map, environment),
                            );
                            conditions.extend(object.required.clone());
                            branches.push(select(
                                row(&subject, &predicate.value, &object),
                                from()?,
                                Vec::new(),
                                conditions,
                            ));
                        } else {
                            // §12.1: "the result of evaluating the join conditions …, combining iterations from
                            // both the child logical source and parent logical sources".
                            let object = term::term(
                                &parent_map.subject_map.term,
                                Position::Subject,
                                parent,
                                base_of(parent_map, environment),
                            );
                            conditions.extend(object.required.clone());
                            let on = r
                                .join_conditions
                                .iter()
                                .map(|j| eq(col(child, &column_name(&j.child)), col(parent, &column_name(&j.parent))))
                                .reduce(build::and)
                                .expect("a join condition");
                            let parent_from = relation(&parent_map.logical_source.iterable, parent, &at, tables)?;
                            branches.push(select(
                                row(&subject, &predicate.value, &object),
                                from()?,
                                vec![join(parent_from, on)],
                                conditions,
                            ));
                        }
                    },
                }
            }
        }
    }
    Ok(branches)
}

/// A logical source as a relation under `alias`.
fn relation(
    iterable: &Iterable,
    alias: &str,
    at: &str,
    tables: &impl Fn(&str) -> ObjectName,
) -> Result<TableFactor, Error> {
    match iterable {
        Iterable::Sql2008Table(name) => Ok(table(tables(name), alias)),
        Iterable::Sql2008Query(text) => {
            let mut statements = Parser::parse_sql(&DuckDbDialect {}, text).map_err(|e| Error::Query {
                at: at.to_owned(),
                reason: e.to_string(),
            })?;
            let Some(sqlparser::ast::Statement::Query(mut q)) = statements.pop().filter(|_| statements.is_empty())
            else {
                return Err(Error::Query {
                    at: at.to_owned(),
                    reason: "expected one SELECT query".to_owned(),
                });
            };
            let mut ctes = CteNames(HashSet::new());
            let _ = q.visit(&mut ctes);
            let _ = VisitMut::visit(&mut *q, &mut Resolve { tables, ctes: &ctes.0 });
            Ok(derived(*q, alias))
        },
    }
}

/// The names the CTEs of a query declare, lower-cased.
struct CteNames(HashSet<String>);

impl Visitor for CteNames {
    type Break = ();

    fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
        if let Some(with) = &q.with {
            self.0
                .extend(with.cte_tables.iter().map(|c| c.alias.name.value.to_lowercase()));
        }
        ControlFlow::Continue(())
    }
}

/// Resolves the tables a query names through the host.
struct Resolve<'a, F: Fn(&str) -> ObjectName> {
    tables: &'a F,
    ctes: &'a HashSet<String>,
}

impl<F: Fn(&str) -> ObjectName> VisitorMut for Resolve<'_, F> {
    type Break = ();

    fn pre_visit_table_factor(&mut self, factor: &mut TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table { name, args: None, .. } = factor
            && let [sqlparser::ast::ObjectNamePart::Identifier(part)] = name.0.as_slice()
            && !self.ctes.contains(&part.value.to_lowercase())
        {
            *name = (self.tables)(&part.value);
        }
        ControlFlow::Continue(())
    }
}
