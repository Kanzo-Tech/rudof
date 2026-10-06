//! Where the RDF terms live in tables.
//!
//! A [`RelationalMapping`] answers two questions in SQL — which pairs a
//! predicate relates, and which triples there are — and the compiler asks
//! nothing else of the data: class extents are derived from `rdf:type` and
//! `rdfs:subClassOf` pairs by the trait itself. Two mappings share the one
//! compiler: [`TripleTable`], a single `(s, p, o)` table that holds arbitrary
//! RDF (and runs the W3C suite), and [`Tables`], ordinary tables described by
//! an RML mapping. Neither carries any product vocabulary.
//!
//! Every relation a mapping returns has fixed column names, each term spread
//! over the four columns of [`crate::validator::sql::term`]:
//!
//! | relation | columns |
//! |---|---|
//! | [`Relation`] (nodes) | `n_k, n_v, n_d, n_l` |
//! | [`PredicateRel`] (edges) | `s_k, s_v, s_d, s_l, o_k, o_v, o_d, o_l` |
//! | triples | the edge columns plus `p`, the predicate IRI |

use crate::validator::sql::ast::{SelectBuilder, boolean, cte, cte_ref, derived, join, query, union, with};
use crate::validator::sql::term::{EncodedTerm, IRI, TermExpr};
use rudof_iri::IriS;
use rudof_rdf::vocab::{RdfVocab, RdfsVocab};
use sqlparser::ast::Query;

/// A relation of nodes, columns `n_k, n_v, n_d, n_l`.
#[derive(Debug, Clone)]
pub(crate) struct Relation(pub(crate) Query);

/// The pairs a predicate relates: subject `s_*`, object `o_*`.
#[derive(Debug, Clone)]
pub(crate) struct PredicateRel(pub(crate) Query);

/// The predicate column of a triples relation.
pub(crate) const PREDICATE_COLUMN: &str = "p";

/// Where the RDF terms live in tables.
///
/// A mapping may answer *bags*: the same pair or triple in several rows (one
/// per source row, or from two rules). The compiler reads them as the sets an
/// RDF graph is, making them distinct where multiplicity would change a result.
pub(crate) trait RelationalMapping {
    /// The subject and object of every triple with `predicate`. `None` when
    /// the mapping has none.
    fn predicate(&self, predicate: &IriS) -> Option<PredicateRel>;

    /// Every triple: the edge columns plus `p`, the predicate IRI as text.
    fn triples(&self) -> Query;

    /// The SHACL instances of `class` (SHACL §1.1): the subjects of
    /// `rdf:type C` for `class` and every class below it by `rdfs:subClassOf`,
    /// as the mapping's own triples state them. `None` when the mapping has
    /// no `rdf:type` triples (an empty extent).
    ///
    /// Derived from [`predicate`](RelationalMapping::predicate), so every
    /// mapping answers it alike; one may override it with a cheaper query.
    fn class_extent(&self, class: &IriS) -> Option<Relation> {
        let types = self.predicate(&RdfVocab::rdf_type())?;
        let class_term = TermExpr::constant(&[IRI.to_owned(), class.as_str().to_owned(), String::new(), String::new()]);
        // WITH RECURSIVE sub AS (class UNION the subclasses of a sub)
        let seed = SelectBuilder::new(class_term.items("c"));
        let sub = match self.predicate(&RdfsVocab::rdfs_subclass_of_str()) {
            Some(sub_class_of) => {
                let step = SelectBuilder::new(TermExpr::columns("e", "s").items("c"))
                    .from(derived(sub_class_of.0, "e"))
                    .join(join(
                        cte_ref("sub", "sub"),
                        TermExpr::columns("e", "o").same(&TermExpr::columns("sub", "c")),
                    ));
                query(union(seed.into_set_expr(), step.into_set_expr(), false))
            },
            None => seed.into_query(),
        };
        let instances = SelectBuilder::new(TermExpr::columns("t", "s").items("n"))
            .distinct()
            .from(derived(types.0, "t"))
            .join(join(
                cte_ref("sub", "sub"),
                TermExpr::columns("t", "o").same(&TermExpr::columns("sub", "c")),
            ))
            .into_query();
        Some(Relation(with(vec![cte("sub", sub)], true, instances)))
    }
}

/// An empty triples relation with the right columns.
pub(crate) fn no_triples() -> Query {
    let none = TermExpr::constant(&EncodedTerm::default());
    let mut items = none.items("s");
    items.push(crate::validator::sql::ast::item(
        crate::validator::sql::ast::string(""),
        PREDICATE_COLUMN,
    ));
    items.extend(none.items("o"));
    SelectBuilder::new(items).filter(boolean(false)).into_query()
}

/// Where the RDF terms live: a triple table, or tables described by RML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlMapping {
    /// One `(s, p, o)` table named `table` (a SQL object name: `triples`,
    /// `main.triples`, `"My Triples"`), every term spread over its columns
    /// `s_k, s_v, p, o_k, o_v, o_d, o_l`.
    TripleTable { table: String },
    /// An RML mapping (Turtle) of ordinary tables.
    /// `schema` (a SQL object name: `schema` or `catalog.schema`) is where the
    /// mapping's one `rml:Source` lives: its unqualified table names resolve
    /// against it.
    Rml { mapping: String, schema: Option<String> },
}
