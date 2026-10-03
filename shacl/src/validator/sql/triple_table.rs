//! [`TripleTable`]: arbitrary RDF in one `(s, p, o)` table.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{SelectBuilder, col, eq, item, parse_object_name, string, table};
use crate::validator::sql::mapping::{PREDICATE_COLUMN, PredicateRel, RelationalMapping};
use crate::validator::sql::term::{TermExpr, encode};
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::Triple;
use sqlparser::ast::{ObjectName, Query};

/// One `(s, p, o)` table: arbitrary RDF, every term in its four columns.
///
/// The table has the columns `s_k, s_v, p, o_k, o_v, o_d, o_l` (a subject has
/// no datatype or language, and `p` is the predicate IRI); [`TripleTable::rows`]
/// encodes a graph into them. `rdf:type` and `rdfs:subClassOf` are ordinary
/// triples here, so class extents follow the data's own class hierarchy.
#[derive(Debug, Clone)]
pub struct TripleTable {
    table: ObjectName,
}

/// The columns of a [`TripleTable`], in order.
pub const TRIPLE_TABLE_COLUMNS: [&str; 7] = ["s_k", "s_v", "p", "o_k", "o_v", "o_d", "o_l"];

impl TripleTable {
    /// The triple table named `table`, a SQL object name (`triples`,
    /// `main.triples`, `"My Triples"`).
    pub fn new(table: &str) -> Result<Self, SqlCompileError> {
        let table = parse_object_name(table).map_err(|e| SqlCompileError::Mapping(format!("the triple table: {e}")))?;
        Ok(Self { table })
    }

    /// The rows of `store` in [`TRIPLE_TABLE_COLUMNS`] order.
    pub fn rows<S>(store: &S) -> Result<Vec<[String; 7]>, SqlCompileError>
    where
        S: NeighsRDF<Term = oxrdf::Term>,
    {
        let triples = store
            .triples()
            .map_err(|e| SqlCompileError::Data(format!("reading the data graph: {e}")))?;
        let mut rows = Vec::new();
        for triple in triples {
            let (subject, predicate, object) = triple.into_components();
            let [s_k, s_v, _, _] = encode(&S::subject_as_term(&subject))?;
            let predicate: IriS = predicate.into();
            let [o_k, o_v, o_d, o_l] = encode(&object)?;
            rows.push([s_k, s_v, predicate.as_str().to_owned(), o_k, o_v, o_d, o_l]);
        }
        Ok(rows)
    }

    fn subject(alias: &str) -> TermExpr {
        let mut t = TermExpr::columns(alias, "s");
        t.datatype = string("");
        t.lang = string("");
        t
    }

    fn object(alias: &str) -> TermExpr {
        TermExpr::columns(alias, "o")
    }

    fn by_predicate(&self, predicate: &str) -> SelectBuilder {
        let mut items = Self::subject("t").items("s");
        items.extend(Self::object("t").items("o"));
        SelectBuilder::new(items)
            .from(table(&self.table, "t"))
            .filter(eq(col("t", PREDICATE_COLUMN), string(predicate)))
    }
}

impl RelationalMapping for TripleTable {
    fn predicate(&self, predicate: &IriS) -> Option<PredicateRel> {
        Some(PredicateRel(self.by_predicate(predicate.as_str()).into_query()))
    }

    fn triples(&self) -> Query {
        let mut items = Self::subject("t").items("s");
        items.push(item(col("t", PREDICATE_COLUMN), PREDICATE_COLUMN));
        items.extend(Self::object("t").items("o"));
        SelectBuilder::new(items).from(table(&self.table, "t")).into_query()
    }
}
