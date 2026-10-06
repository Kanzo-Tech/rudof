//! [`Tables`]: ordinary tables, as an RML mapping describes them.
//!
//! This module holds the *model* the RML reader ([`crate::validator::sql::rml`])
//! builds — sources, logical views and triple rules — and its translation to
//! SQL. Each rule yields one `(subject, object)` relation for one predicate;
//! a predicate's relation is the `UNION ALL` of its rules'.
//!
//! - A table source is `SELECT * FROM table`; a logical view is a `SELECT` of
//!   its fields over its source, joined (`JOIN` / `LEFT JOIN`) to its parent
//!   views, with each view's columns named after its fields.
//! - A term built from a reference reads that column, as text; a reference
//!   that is `NULL` generates no term, so its rows are filtered out.
//! - A literal with neither datatype nor language is the *natural RDF literal*
//!   of its column's SQL type, which the dialect determines.
//! - A referencing object map joins the parent triples map's source on its
//!   join conditions, or reads the parent subject from the same row when
//!   there are none (the sources being the same).

use crate::validator::sql::ast::{
    SelectBuilder, and_all, case, col, compare, derived, eq, is_not_null, item, join, left_join, query, string, table,
    union_all_of,
};
use crate::validator::sql::dialect::Dialect;
use crate::validator::sql::mapping::{PREDICATE_COLUMN, PredicateRel, RelationalMapping, no_triples};
use crate::validator::sql::term::{BLANK, EncodedTerm, IRI, LITERAL, RDF_LANG_STRING, TermExpr};
use rudof_iri::IriS;
use sqlparser::ast::{BinaryOperator, Expr, ObjectName, Query, SelectItem};

/// Where rows come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// A table, by its SQL object name.
    Table(ObjectName),
    /// An RML logical view.
    View(Box<View>),
    /// No source: a triples map whose terms are all constant has one row.
    Unit,
}

/// An RML logical view: fields over a source, and joins to parent views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) on: Source,
    /// `(field name, reference on the source)`.
    pub(crate) fields: Vec<(String, String)>,
    pub(crate) joins: Vec<ViewJoin>,
}

/// A logical view join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewJoin {
    /// `rml:leftJoin` rather than `rml:innerJoin`.
    pub(crate) left: bool,
    pub(crate) parent: Box<View>,
    /// `(child reference, parent reference)`: a field of the child view, and
    /// one of the parent view.
    pub(crate) conditions: Vec<(String, String)>,
    /// `(field name, reference on the parent view)`.
    pub(crate) fields: Vec<(String, String)>,
}

/// What a term map evaluates: a reference (a column), or a constant term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Reference(String),
    Constant(EncodedTerm),
}

/// The kind of term a reference-valued term map generates.
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
    pub(crate) datatype: Option<Value>,
    pub(crate) language: Option<Value>,
    /// `rml:baseIRI`: what a relative IRI is resolved against.
    pub(crate) base_iri: Option<String>,
}

/// The object of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObjectRule {
    Term(TermRule),
    /// A referencing object map: the parent triples map's subject.
    Parent {
        source: Source,
        subject: TermRule,
        /// `(child reference, parent reference)`; none means the same row.
        conditions: Vec<(String, String)>,
    },
}

/// The triples of one predicate from one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    pub(crate) predicate: String,
    pub(crate) source: Source,
    pub(crate) subject: TermRule,
    pub(crate) object: ObjectRule,
}

/// Ordinary tables, as an RML mapping describes them; see the module docs.
#[derive(Debug, Clone)]
pub(crate) struct Tables<D> {
    rules: Vec<Rule>,
    /// What an unqualified table name is qualified with.
    schema: Option<ObjectName>,
    dialect: D,
}

const CHILD: &str = "t";
const PARENT: &str = "p";

impl<D: Dialect> Tables<D> {
    pub(crate) fn new(rules: Vec<Rule>, schema: Option<ObjectName>, dialect: D) -> Self {
        Self { rules, schema, dialect }
    }

    /// The predicates the mapping has triples for.
    #[cfg(test)]
    pub(crate) fn predicates(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.rules.iter().map(|r| r.predicate.as_str()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// `name`, qualified with the schema when it has a single part.
    fn qualified(&self, name: &ObjectName) -> ObjectName {
        match &self.schema {
            Some(schema) if name.0.len() == 1 => ObjectName(schema.0.iter().chain(name.0.iter()).cloned().collect()),
            _ => name.clone(),
        }
    }

    /// The rows of a source, its columns named as the source names them.
    fn source_query(&self, source: &Source) -> Query {
        match source {
            Source::Table(name) => SelectBuilder::new(vec![SelectItem::Wildcard(Default::default())])
                .from(table(&self.qualified(name), "s"))
                .into_query(),
            Source::Unit => SelectBuilder::new(vec![item(crate::validator::sql::ast::number(1), "unit")]).into_query(),
            Source::View(view) => self.view_query(view),
        }
    }

    fn view_query(&self, view: &View) -> Query {
        let mut items: Vec<SelectItem> = view
            .fields
            .iter()
            .map(|(name, reference)| item(col("s", reference), name))
            .collect();
        let child_field = |reference: &str| -> Expr {
            // A join condition's child reference names a field of the child view.
            match view.fields.iter().find(|(name, _)| name == reference) {
                Some((_, column)) => col("s", column),
                None => col("s", reference),
            }
        };
        let mut select = SelectBuilder::new(Vec::new()).from(derived(self.source_query(&view.on), "s"));
        for (i, j) in view.joins.iter().enumerate() {
            let alias = format!("j{i}");
            items.extend(
                j.fields
                    .iter()
                    .map(|(name, reference)| item(col(&alias, reference), name)),
            );
            let on = and_all(
                j.conditions
                    .iter()
                    .map(|(child, parent)| eq(child_field(child), col(&alias, parent))),
            );
            let parent = derived(self.view_query(&j.parent), &alias);
            select = select.join(if j.left {
                left_join(parent, on)
            } else {
                join(parent, on)
            });
        }
        select.with_projection(items).into_query()
    }

    /// The term a term map generates on the row `alias`.
    fn term(&self, rule: &TermRule, alias: &str) -> TermExpr {
        let reference = match &rule.value {
            Value::Constant(term) => return TermExpr::constant(term),
            Value::Reference(reference) => col(alias, reference),
        };
        let lex = self.dialect.to_text(reference.clone());
        let attribute = |value: &Value| match value {
            Value::Constant(term) => string(&term[1]),
            Value::Reference(r) => self.dialect.to_text(col(alias, r)),
        };
        match rule.term_type {
            TermType::Iri => TermExpr {
                kind: string(IRI),
                lex: match &rule.base_iri {
                    // RML-Core §12.2: a value that is not an absolute IRI is
                    // prepended with the base IRI.
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
            TermType::Literal => {
                // R2RML §10.2: an overriding datatype keeps the natural lexical form.
                let lex = self.dialect.natural_lexical(reference.clone());
                match (&rule.language, &rule.datatype) {
                    (Some(language), _) => TermExpr {
                        kind: string(LITERAL),
                        lex,
                        datatype: string(RDF_LANG_STRING),
                        lang: crate::validator::sql::ast::function("LOWER", vec![attribute(language)]),
                    },
                    (None, Some(datatype)) => TermExpr {
                        kind: string(LITERAL),
                        lex,
                        datatype: attribute(datatype),
                        lang: string(""),
                    },
                    (None, None) => TermExpr {
                        kind: string(LITERAL),
                        lex,
                        datatype: self.dialect.natural_datatype(reference),
                        lang: string(""),
                    },
                }
            },
        }
    }

    /// `NOT NULL` for every reference a term map reads: a missing value
    /// generates no term (RML-Core §12.2).
    fn present(rule: &TermRule, alias: &str) -> Vec<Expr> {
        [Some(&rule.value), rule.datatype.as_ref(), rule.language.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(|v| match v {
                Value::Reference(r) => Some(is_not_null(col(alias, r))),
                Value::Constant(_) => None,
            })
            .collect()
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
