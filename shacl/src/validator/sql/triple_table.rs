//! [`TripleTable`]: arbitrary RDF in one `(s, p, o)` table.

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{SelectBuilder, col, eq, item, parse_object_name, string, table};
use crate::validator::sql::mapping::{PREDICATE_COLUMN, PredicateRel, RelationalMapping};
use crate::validator::sql::term::TermExpr;
use rudof_iri::IriS;
use sqlparser::ast::{ObjectName, Query};

/// One `(s, p, o)` table: arbitrary RDF, every term in its four columns.
///
/// The table has the columns `s_k, s_v, p, o_k, o_v, o_d, o_l` (a subject has
/// no datatype or language, and `p` is the predicate IRI). `rdf:type` and `rdfs:subClassOf` are ordinary
/// triples here, so class extents follow the data's own class hierarchy.
#[derive(Debug, Clone)]
pub(crate) struct TripleTable {
    table: ObjectName,
}

impl TripleTable {
    /// The triple table named `table`, a SQL object name (`triples`,
    /// `main.triples`, `"My Triples"`).
    pub(crate) fn new(table: &str) -> Result<Self, SqlCompileError> {
        let table = parse_object_name(table).map_err(|e| SqlCompileError::Mapping(format!("the triple table: {e}")))?;
        Ok(Self { table })
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
