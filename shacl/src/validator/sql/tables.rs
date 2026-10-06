//! [`Tables`]: ordinary tables, as an R2RML mapping describes them.
//!
//! This module holds the *model* the R2RML reader ([`crate::validator::sql::r2rml`])
//! builds — logical tables and triple rules — and its translation to SQL,
//! the unfolding Ontop does. Each rule yields one `(subject, object)`
//! relation for one predicate; a predicate's relation is the `UNION ALL` of
//! its rules'.
//!
//! - A logical table is `SELECT * FROM table`, or an R2RML view's query as a
//!   derived table (R2RML §5.2: its result is the logical table).
//! - A term map's value is its column, its template filled with column values
//!   (R2RML §7.3; IRI-safe when it generates an IRI), or a constant. Values
//!   are *natural RDF lexical forms* (R2RML §10.2), which the dialect writes.
//! - A column that is `NULL` generates no term (R2RML §11), so its rows are
//!   filtered out.
//! - A literal with neither datatype nor language is the *natural RDF literal*
//!   of its column's SQL type.
//! - A referencing object map joins the parent triples map's logical table on
//!   its join conditions, or reads the parent subject from the same row when
//!   there are none (the logical tables being the same, R2RML §8).

use crate::validator::sql::ast::{
    SelectBuilder, and_all, case, col, compare, derived, eq, is_not_null, item, join, query, string, table,
    union_all_of,
};
use crate::validator::sql::dialect::Dialect;
use crate::validator::sql::mapping::{PREDICATE_COLUMN, PredicateRel, RelationalMapping, no_triples};
use crate::validator::sql::term::{BLANK, EncodedTerm, IRI, LITERAL, RDF_LANG_STRING, TermExpr};
use rudof_iri::IriS;
use sqlparser::ast::{BinaryOperator, Expr, ObjectName, Query, SelectItem};

/// A logical table (R2RML §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// A table or view, by its SQL object name (`rr:tableName`).
    Table(ObjectName),
    /// An R2RML view (`rr:sqlQuery`), its table names already qualified.
    Query(Box<Query>),
}

/// What a term map evaluates (R2RML §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Column(String),
    Template(Vec<Part>),
    Constant(EncodedTerm),
}

/// A piece of a string template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Part {
    Text(String),
    Column(String),
}

/// The kind of term a term map generates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TermType {
    Iri,
    BlankNode,
    Literal,
}

/// A term map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TermRule {
    pub(crate) value: Value,
    pub(crate) term_type: TermType,
    /// `rr:datatype`, an IRI.
    pub(crate) datatype: Option<String>,
    /// `rr:language`, a language tag.
    pub(crate) language: Option<String>,
}

/// The object of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObjectRule {
    Term(TermRule),
    /// A referencing object map: the parent triples map's subject.
    Parent {
        source: Source,
        subject: TermRule,
        /// `(child column, parent column)`; none means the same row.
        conditions: Vec<(String, String)>,
    },
}

/// The triples of one predicate from one logical table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    pub(crate) predicate: String,
    pub(crate) source: Source,
    pub(crate) subject: TermRule,
    pub(crate) object: ObjectRule,
}

/// Ordinary tables, as an R2RML mapping describes them; see the module docs.
#[derive(Debug, Clone)]
pub(crate) struct Tables<D> {
    rules: Vec<Rule>,
    /// What an unqualified table name is qualified with.
    schema: Option<ObjectName>,
    /// What a relative IRI is resolved against (R2RML §11).
    base_iri: Option<String>,
    dialect: D,
}

const CHILD: &str = "t";
const PARENT: &str = "p";

impl<D: Dialect> Tables<D> {
    pub(crate) fn new(rules: Vec<Rule>, schema: Option<ObjectName>, base_iri: Option<String>, dialect: D) -> Self {
        Self {
            rules,
            schema,
            base_iri,
            dialect,
        }
    }

    /// The predicates the mapping has triples for.
    #[cfg(test)]
    pub(crate) fn predicates(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.rules.iter().map(|r| r.predicate.as_str()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The rows of a logical table, its columns named as it names them.
    fn source_query(&self, source: &Source) -> Query {
        match source {
            Source::Table(name) => {
                let name = match &self.schema {
                    Some(schema) if name.0.len() == 1 => {
                        ObjectName(schema.0.iter().chain(name.0.iter()).cloned().collect())
                    },
                    _ => name.clone(),
                };
                SelectBuilder::new(vec![SelectItem::Wildcard(Default::default())])
                    .from(table(&name, "s"))
                    .into_query()
            },
            Source::Query(query) => (**query).clone(),
        }
    }

    /// The natural RDF lexical form of `alias.column`.
    fn lexical(&self, alias: &str, column: &str) -> Expr {
        self.dialect.natural_lexical(col(alias, column))
    }

    /// The string a column- or template-valued term map yields on the row
    /// `alias`; template values are IRI-safe when it generates an IRI.
    fn text(&self, value: &Value, term_type: TermType, alias: &str) -> Expr {
        match value {
            Value::Column(column) => self.lexical(alias, column),
            Value::Template(parts) => parts
                .iter()
                .map(|part| match part {
                    Part::Text(text) => string(text),
                    Part::Column(column) if term_type == TermType::Iri => {
                        self.dialect.iri_safe(self.lexical(alias, column))
                    },
                    Part::Column(column) => self.lexical(alias, column),
                })
                .reduce(|a, b| compare(a, BinaryOperator::StringConcat, b))
                .unwrap_or_else(|| string("")),
            Value::Constant(term) => string(&term[1]),
        }
    }

    /// The term a term map generates on the row `alias` (R2RML §11).
    fn term(&self, rule: &TermRule, alias: &str) -> TermExpr {
        if let Value::Constant(term) = &rule.value {
            return TermExpr::constant(term);
        }
        let lex = self.text(&rule.value, rule.term_type, alias);
        match rule.term_type {
            TermType::Iri => TermExpr {
                kind: string(IRI),
                lex: match &self.base_iri {
                    // A value that is not an absolute IRI is appended to the
                    // base IRI.
                    Some(base) => case(
                        vec![(
                            self.dialect
                                .regex_match(lex.clone(), "^[A-Za-z][A-Za-z0-9+.-]*:", None)
                                .unwrap_or_else(|_| crate::validator::sql::ast::boolean(true)),
                            lex.clone(),
                        )],
                        compare(string(base), BinaryOperator::StringConcat, lex),
                    ),
                    None => lex,
                },
                datatype: string(""),
                lang: string(""),
            },
            TermType::BlankNode => TermExpr {
                kind: string(BLANK),
                lex,
                datatype: string(""),
                lang: string(""),
            },
            TermType::Literal => TermExpr {
                kind: string(LITERAL),
                datatype: match (&rule.language, &rule.datatype, &rule.value) {
                    (Some(_), _, _) => string(RDF_LANG_STRING),
                    (None, Some(datatype), _) => string(datatype),
                    (None, None, Value::Column(column)) => self.dialect.natural_datatype(col(alias, column)),
                    (None, None, _) => string(crate::validator::sql::term::XSD_STRING),
                },
                lang: string(&rule.language.as_deref().unwrap_or_default().to_lowercase()),
                lex,
            },
        }
    }

    /// `NOT NULL` for every column a term map reads: a `NULL` generates no
    /// term (R2RML §11).
    fn present(rule: &TermRule, alias: &str) -> Vec<Expr> {
        let columns: Vec<&str> = match &rule.value {
            Value::Column(column) => vec![column],
            Value::Template(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    Part::Column(column) => Some(column.as_str()),
                    Part::Text(_) => None,
                })
                .collect(),
            Value::Constant(_) => Vec::new(),
        };
        columns.into_iter().map(|c| is_not_null(col(alias, c))).collect()
    }

    /// The `(s, o)` pairs of one rule.
    fn edges(&self, rule: &Rule) -> SelectBuilder {
        let subject = self.term(&rule.subject, CHILD);
        let mut conditions = Self::present(&rule.subject, CHILD);
        let mut select = SelectBuilder::new(Vec::new()).from(derived(self.source_query(&rule.source), CHILD));
        let object = match &rule.object {
            ObjectRule::Term(object) => {
                conditions.extend(Self::present(object, CHILD));
                self.term(object, CHILD)
            },
            ObjectRule::Parent {
                subject: parent_subject,
                conditions: joins,
                ..
            } if joins.is_empty() => {
                conditions.extend(Self::present(parent_subject, CHILD));
                self.term(parent_subject, CHILD)
            },
            ObjectRule::Parent {
                source,
                subject: parent_subject,
                conditions: joins,
            } => {
                let on = and_all(
                    joins
                        .iter()
                        .map(|(child, parent)| eq(col(CHILD, child), col(PARENT, parent))),
                );
                select = select.join(join(derived(self.source_query(source), PARENT), on));
                conditions.extend(Self::present(parent_subject, PARENT));
                self.term(parent_subject, PARENT)
            },
        };
        let mut items = subject.items("s");
        items.extend(object.items("o"));
        let mut select = select.with_projection(items);
        for condition in conditions {
            select = select.filter(condition);
        }
        select
    }
}

impl<D: Dialect> RelationalMapping for Tables<D> {
    fn predicate(&self, predicate: &IriS) -> Option<PredicateRel> {
        let bodies: Vec<_> = self
            .rules
            .iter()
            .filter(|r| r.predicate == predicate.as_str())
            .map(|r| self.edges(r).into_set_expr())
            .collect();
        union_all_of(bodies, true).map(|body| PredicateRel(query(body)))
    }

    fn triples(&self) -> Query {
        let bodies: Vec<_> = self
            .rules
            .iter()
            .map(|r| {
                let mut items = TermExpr::columns("e", "s").items("s");
                items.push(item(string(&r.predicate), PREDICATE_COLUMN));
                items.extend(TermExpr::columns("e", "o").items("o"));
                SelectBuilder::new(items)
                    .from(derived(self.edges(r).into_query(), "e"))
                    .into_set_expr()
            })
            .collect();
        union_all_of(bodies, true).map_or_else(no_triples, query)
    }
}
