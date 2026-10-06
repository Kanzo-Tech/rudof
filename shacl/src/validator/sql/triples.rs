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
    SelectBuilder, col, cte, cte_ref, derived, eq, item, join, parse_object_name, query, string, table, union, with,
};
use crate::validator::sql::term::{IRI, TermExpr};
use rudof_iri::IriS;
use rudof_rdf::vocab::{RdfVocab, RdfsVocab};
use sqlparser::ast::{ObjectName, Query};

/// The predicate column.
pub(crate) const PREDICATE_COLUMN: &str = "p";

/// The `(s, p, o)` relation the data is read from.
#[derive(Debug, Clone)]
pub(crate) struct Triples {
    table: ObjectName,
}

impl Triples {
    /// The relation named `table`, a SQL object name (`triples`,
    /// `"job".triples`, `"My Triples"`).
    pub(crate) fn new(table: &str) -> Result<Self, SqlCompileError> {
        let table = parse_object_name(table).map_err(|e| SqlCompileError::Table(format!("{table}: {e}")))?;
        Ok(Self { table })
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
