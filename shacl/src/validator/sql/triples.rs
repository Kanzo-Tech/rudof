//! [`Triples`]: the data, as one `(s, p, o)` relation.
//!
//! The relation has the columns `s_k, s_v, p, o_k, o_v, o_d, o_l`: each term
//! spread over the columns of [`crate::validator::sql::term`] (a subject has
//! no datatype or language) and `p` the predicate IRI. It may be a table or
//! a view, over any tables a host holds; the engine reads nothing else.
//! `rdf:type` and `rdfs:subClassOf` are ordinary triples here, so class
//! extents follow the data's own class hierarchy.
//!
//! What the relations built here hold:
//!
//! | relation | columns |
//! |---|---|
//! | [`predicate`](Triples::predicate) (pairs) | `s_k, s_v, s_d, s_l, o_k, o_v, o_d, o_l` |
//! | [`class_extent`](Triples::class_extent) (nodes) | `n_k, n_v, n_d, n_l` |
//! | [`all`](Triples::all) | the pair columns plus `p` |
//!
//! The relation may hold a triple twice (a view over several source rows);
//! the compiler reads it as the set an RDF graph is, making it distinct where
//! multiplicity would change a result.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{
    SelectBuilder, and_all, col, cte, cte_ref, derived, eq, exists, item, join, number, parse_object_name, query,
    string, table, union, with,
};
use crate::validator::sql::term::{IRI, TermExpr};
use rudof_iri::IriS;
use rudof_rdf::vocab::{RdfVocab, RdfsVocab};
use sqlparser::ast::{ObjectName, Query, SelectItem, TableFactor};

/// The predicate column.
pub(crate) const PREDICATE_COLUMN: &str = "p";

/// The columns of the relation, in order.
pub(crate) const COLUMNS: [&str; 7] = ["s_k", "s_v", "p", "o_k", "o_v", "o_d", "o_l"];

/// The relation a SQL object name names (`triples`, `"job".triples`,
/// `"My Triples"`).
pub(crate) fn relation(name: &str) -> Result<ObjectName, SqlCompileError> {
    parse_object_name(name).map_err(|e| SqlCompileError::Relation(format!("{name}: {e}")))
}

/// The `(s, p, o)` relation the data is read from, and the relation of the
/// focus nodes validation is restricted to, if any.
#[derive(Debug, Clone)]
pub(crate) struct Triples {
    table: ObjectName,
    focus: Option<ObjectName>,
}

impl Triples {
    /// The relation named `table`, a SQL object name (`triples`,
    /// `"job".triples`, `"My Triples"`), and `focus`, the name of a relation
    /// whose `s_k, s_v` are nodes, spelled as the triples' subjects.
    pub(crate) fn new(table: &str, focus: Option<&str>) -> Result<Self, SqlCompileError> {
        Ok(Self {
            table: relation(table)?,
            focus: focus.map(relation).transpose()?,
        })
    }

    /// The nodes of the focus relation, `n_k, n_v, n_d, n_l`; `None` when
    /// validation is not restricted.
    pub(crate) fn scope(&self) -> Option<Query> {
        let focus = self.focus.as_ref()?;
        Some(
            SelectBuilder::new(Self::subject("t").items("n"))
                .from(table(focus, "t"))
                .into_query(),
        )
    }

    fn subject(alias: &str) -> TermExpr {
        let mut t = TermExpr::columns(alias, "s");
        t.datatype = string("");
        t.lang = string("");
        t
    }

    /// The subject and object of every triple with `predicate`.
    pub(crate) fn predicate(&self, predicate: &IriS) -> Query {
        let mut items = Self::subject("t").items("s");
        items.extend(TermExpr::columns("t", "o").items("o"));
        SelectBuilder::new(items)
            .from(table(&self.table, "t"))
            .filter(eq(col("t", PREDICATE_COLUMN), string(predicate.as_str())))
            .into_query()
    }

    /// Every triple: the pair columns plus `p`.
    pub(crate) fn all(&self) -> Query {
        let mut items = Self::subject("t").items("s");
        items.push(item(col("t", PREDICATE_COLUMN), PREDICATE_COLUMN));
        items.extend(TermExpr::columns("t", "o").items("o"));
        SelectBuilder::new(items).from(table(&self.table, "t")).into_query()
    }

    /// The rows of the relation, in its seven columns, whose triple is one of
    /// the triples relation `among`.
    pub(crate) fn among(&self, among: TableFactor) -> Query {
        let items = COLUMNS.iter().map(|c| item(col("t", c), c)).collect();
        let same = and_all([
            Self::subject("t").same(&TermExpr::columns("m", "f")),
            eq(col("t", PREDICATE_COLUMN), col("m", PREDICATE_COLUMN)),
            TermExpr::columns("t", "o").same(&TermExpr::columns("m", "v")),
        ]);
        SelectBuilder::new(items)
            .from(table(&self.table, "t"))
            .filter(exists(
                SelectBuilder::new(vec![SelectItem::UnnamedExpr(number(1))])
                    .from(among)
                    .filter(same)
                    .into_query(),
            ))
            .into_query()
    }

    /// The SHACL instances of `class` (SHACL §1.1): the subjects of
    /// `rdf:type C` for `class` and every class below it by `rdfs:subClassOf`.
    pub(crate) fn class_extent(&self, class: &IriS) -> Query {
        let class_term = TermExpr::constant(&[IRI.to_owned(), class.as_str().to_owned(), String::new(), String::new()]);
        // WITH RECURSIVE sub AS (class UNION the subclasses of a sub)
        let seed = SelectBuilder::new(class_term.items("c"));
        let step = SelectBuilder::new(TermExpr::columns("e", "s").items("c"))
            .from(derived(self.predicate(&RdfsVocab::rdfs_subclass_of_str()), "e"))
            .join(join(
                cte_ref("sub", "sub"),
                TermExpr::columns("e", "o").same(&TermExpr::columns("sub", "c")),
            ));
        let sub = query(union(seed.into_set_expr(), step.into_set_expr(), false));
        let instances = SelectBuilder::new(TermExpr::columns("t", "s").items("n"))
            .distinct()
            .from(derived(self.predicate(&RdfVocab::rdf_type()), "t"))
            .join(join(
                cte_ref("sub", "sub"),
                TermExpr::columns("t", "o").same(&TermExpr::columns("sub", "c")),
            ))
            .into_query();
        with(vec![cte("sub", sub)], true, instances)
    }
}
