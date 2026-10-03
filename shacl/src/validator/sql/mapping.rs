//! Where the RDF terms live in tables.
//!
//! A [`RelationalMapping`] answers three questions in SQL — which nodes are
//! instances of a class, which pairs a predicate relates, and which triples
//! there are — and the compiler asks nothing else of the data. Two mappings
//! share the one compiler: [`TripleTable`], a single `(s, p, o)` table that
//! holds arbitrary RDF (and runs the W3C suite), and [`Tables`], an R2RML-like
//! mapping of ordinary tables that a host describes in a serde DTO. Neither
//! carries any product vocabulary.
//!
//! Every relation a mapping returns has fixed column names, each term spread
//! over the four columns of [`crate::validator::sql::term`]:
//!
//! | relation | columns |
//! |---|---|
//! | [`Relation`] (nodes) | `n_k, n_v, n_d, n_l` |
//! | [`PredicateRel`] (edges) | `s_k, s_v, s_d, s_l, o_k, o_v, o_d, o_l` |
//! | triples | the edge columns plus `p`, the predicate IRI |

use crate::validator::sql::SqlCompileError;
use crate::validator::sql::ast::{
    SelectBuilder, and_all, boolean, case, col, cte, cte_ref, derived, eq, function, is_not_null, item, join, query,
    string, table, union, union_all_of, with,
};
use crate::validator::sql::dialect::SqlDialect;
use crate::validator::sql::term::{BLANK, EncodedTerm, IRI, LITERAL, RDF_LANG_STRING, TermExpr, XSD_STRING, encode};
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::Triple;
use rudof_rdf::vocab::{RdfVocab, RdfsVocab};
use serde::{Deserialize, Serialize};
use sqlparser::ast::Query;
use std::collections::{BTreeMap, BTreeSet};

/// A relation of nodes, columns `n_k, n_v, n_d, n_l`.
#[derive(Debug, Clone)]
pub struct Relation(pub Query);

/// The pairs a predicate relates: subject `s_*`, object `o_*`.
#[derive(Debug, Clone)]
pub struct PredicateRel(pub Query);

/// Where the RDF terms live in tables.
pub trait RelationalMapping {
    /// The SHACL instances of `class`: the subjects of `rdf:type class`,
    /// closed under `rdfs:subClassOf` as far as the mapping carries it. `None`
    /// when the mapping has none (an empty extent).
    fn class_extent(&self, class: &IriS) -> Option<Relation>;

    /// The subject and object of every triple with `predicate`. `None` when
    /// the mapping has none.
    fn predicate(&self, predicate: &IriS) -> Option<PredicateRel>;

    /// Every triple: the edge columns plus `p`, the predicate IRI as text.
    fn triples(&self) -> Query;

    /// Every subject of a triple.
    fn subjects(&self) -> Relation {
        let t = TermExpr::columns("t", "s");
        Relation(
            SelectBuilder::new(t.items("n"))
                .distinct()
                .from(derived(self.triples(), "t"))
                .into_query(),
        )
    }
}

/// The predicate column of a triples relation.
pub const PREDICATE_COLUMN: &str = "p";

// ---------------------------------------------------------------------------
// TripleTable
// ---------------------------------------------------------------------------

/// One `(s, p, o)` table: arbitrary RDF, every term in its four columns.
///
/// The table has the columns `s_k, s_v, p, o_k, o_v, o_d, o_l` (a subject has
/// no datatype or language, and `p` is the predicate IRI); [`TripleTable::rows`]
/// encodes a graph into them. `rdf:type` and `rdfs:subClassOf` are ordinary
/// triples here, so class extents follow the data's own class hierarchy.
#[derive(Debug, Clone)]
pub struct TripleTable {
    table: String,
}

/// The columns of a [`TripleTable`], in order.
pub const TRIPLE_TABLE_COLUMNS: [&str; 7] = ["s_k", "s_v", "p", "o_k", "o_v", "o_d", "o_l"];

impl TripleTable {
    /// The triple table named `table` (dotted for a schema: `"main.triples"`).
    pub fn new(table: impl Into<String>) -> Self {
        Self { table: table.into() }
    }

    pub fn table(&self) -> &str {
        &self.table
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
    fn class_extent(&self, class: &IriS) -> Option<Relation> {
        // WITH RECURSIVE sub AS (class UNION subclasses of sub) instances of sub.
        let sub_of = RdfsVocab::rdfs_subclass_of_str().as_str().to_owned();
        let rdf_type = RdfVocab::rdf_type().as_str().to_owned();
        let seed = SelectBuilder::new(vec![item(string(IRI), "c_k"), item(string(class.as_str()), "c_v")]);
        let step = SelectBuilder::new(vec![item(col("t", "s_k"), "c_k"), item(col("t", "s_v"), "c_v")])
            .from(table(&self.table, "t"))
            .join(join(
                cte_ref("sub", "sub"),
                and_all([
                    eq(col("t", "o_k"), col("sub", "c_k")),
                    eq(col("t", "o_v"), col("sub", "c_v")),
                ]),
            ))
            .filter(eq(col("t", PREDICATE_COLUMN), string(&sub_of)));
        let sub = query(union(seed.into_set_expr(), step.into_set_expr(), false));
        let instances = SelectBuilder::new(Self::subject("t").items("n"))
            .distinct()
            .from(table(&self.table, "t"))
            .join(join(
                cte_ref("sub", "sub"),
                and_all([
                    eq(col("t", "o_k"), col("sub", "c_k")),
                    eq(col("t", "o_v"), col("sub", "c_v")),
                ]),
            ))
            .filter(eq(col("t", PREDICATE_COLUMN), string(&rdf_type)))
            .into_query();
        Some(Relation(with(vec![cte("sub", sub)], true, instances)))
    }

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

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// The kind of term a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TermType {
    #[default]
    #[serde(rename = "IRI")]
    Iri,
    #[serde(rename = "BlankNode")]
    BlankNode,
    #[serde(rename = "Literal")]
    Literal,
}

/// How a column becomes an RDF term (an R2RML term map, column-valued).
///
/// The column's value, as text, is the IRI, the blank node label or the
/// literal's lexical form. A literal takes its datatype and language from a
/// constant or from another column; a literal with neither is an
/// `xsd:string`, and one with a language is an `rdf:langString`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TermMap {
    pub column: String,
    #[serde(default)]
    pub term_type: TermType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_column: Option<String>,
}

impl TermMap {
    /// An IRI held in `column`.
    pub fn iri(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            ..Self::default()
        }
    }

    /// A literal of `datatype` (an `xsd:string` when `None`) held in `column`.
    pub fn literal(column: impl Into<String>, datatype: Option<&str>) -> Self {
        Self {
            column: column.into(),
            term_type: TermType::Literal,
            datatype: datatype.map(str::to_owned),
            ..Self::default()
        }
    }

    fn term<D: SqlDialect + ?Sized>(&self, dialect: &D, alias: &str) -> TermExpr {
        let c = |name: &str| col(alias, name);
        let lex = dialect.to_text(c(&self.column));
        match self.term_type {
            TermType::Iri | TermType::BlankNode => TermExpr {
                kind: string(if self.term_type == TermType::Iri { IRI } else { BLANK }),
                lex,
                datatype: string(""),
                lang: string(""),
            },
            TermType::Literal => {
                let lang = match (&self.language, &self.language_column) {
                    (Some(l), _) => string(&l.to_lowercase()),
                    (None, Some(column)) => function(
                        "LOWER",
                        vec![function("COALESCE", vec![dialect.to_text(c(column)), string("")])],
                    ),
                    (None, None) => string(""),
                };
                let datatype = match (
                    &self.datatype,
                    &self.datatype_column,
                    &self.language,
                    &self.language_column,
                ) {
                    (Some(d), _, _, _) => string(d),
                    (None, Some(column), _, _) => dialect.to_text(c(column)),
                    (None, None, Some(_), _) => string(RDF_LANG_STRING),
                    (None, None, None, Some(_)) => case(
                        vec![(eq(lang.clone(), string("")), string(XSD_STRING))],
                        string(RDF_LANG_STRING),
                    ),
                    (None, None, None, None) => string(XSD_STRING),
                };
                TermExpr {
                    kind: string(LITERAL),
                    lex,
                    datatype,
                    lang,
                }
            },
        }
    }
}

/// A class whose instances are the rows of a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassMap {
    /// The class IRI.
    pub class: String,
    /// The table (dotted for a schema).
    pub table: String,
    /// The instance each row is.
    pub subject: TermMap,
}

/// A predicate whose triples are the rows of a table: a literal column of an
/// entity table, or an edge table `(src, dst)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PropertyMap {
    /// The predicate IRI.
    pub predicate: String,
    pub table: String,
    pub subject: TermMap,
    pub object: TermMap,
}

/// `sub rdfs:subClassOf super`, declared by the mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubClassOf {
    pub sub: String,
    #[serde(rename = "super")]
    pub sup: String,
}

/// The DTO a host sends to describe its tables.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TablesSpec {
    #[serde(default)]
    pub classes: Vec<ClassMap>,
    #[serde(default)]
    pub properties: Vec<PropertyMap>,
    #[serde(default)]
    pub sub_class_of: Vec<SubClassOf>,
}

/// An R2RML-like mapping of ordinary tables: classes to tables and subject
/// columns, predicates to literal columns or edge tables.
///
/// `rdf:type` triples are the class tables' rows, and `rdfs:subClassOf` the
/// declared [`SubClassOf`] pairs; a class extent includes the extents of its
/// declared subclasses.
#[derive(Debug, Clone)]
pub struct Tables<D> {
    spec: TablesSpec,
    dialect: D,
}

impl<D: SqlDialect> Tables<D> {
    pub fn new(spec: TablesSpec, dialect: D) -> Self {
        Self { spec, dialect }
    }

    /// Reads the DTO from JSON.
    pub fn from_json(json: &str, dialect: D) -> Result<Self, SqlCompileError> {
        let spec: TablesSpec =
            serde_json::from_str(json).map_err(|e| SqlCompileError::Mapping(format!("invalid mapping JSON: {e}")))?;
        Ok(Self::new(spec, dialect))
    }

    pub fn spec(&self) -> &TablesSpec {
        &self.spec
    }

    /// `class` and every class declared below it.
    fn subclasses(&self, class: &str) -> BTreeSet<String> {
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for s in &self.spec.sub_class_of {
            children.entry(s.sup.as_str()).or_default().push(s.sub.as_str());
        }
        let mut seen = BTreeSet::new();
        let mut pending = vec![class.to_owned()];
        while let Some(c) = pending.pop() {
            if seen.insert(c.clone()) {
                pending.extend(children.get(c.as_str()).into_iter().flatten().map(|s| (*s).to_owned()));
            }
        }
        seen
    }

    fn not_null(&self, alias: &str, map: &TermMap) -> sqlparser::ast::Expr {
        is_not_null(col(alias, &map.column))
    }

    fn edges(
        &self,
        table_name: &str,
        subject: &TermMap,
        object: TermExpr,
        filter: bool,
        object_map: Option<&TermMap>,
    ) -> SelectBuilder {
        let mut items = subject.term(&self.dialect, "t").items("s");
        items.extend(object.items("o"));
        let mut select = SelectBuilder::new(items)
            .from(table(table_name, "t"))
            .filter(self.not_null("t", subject));
        if filter && let Some(map) = object_map {
            select = select.filter(self.not_null("t", map));
        }
        select
    }

    /// Every predicate the mapping has triples for, with their edges.
    fn all_edges(&self) -> Vec<(String, SelectBuilder)> {
        let mut out = Vec::new();
        let rdf_type = RdfVocab::rdf_type().as_str().to_owned();
        for c in &self.spec.classes {
            let object = TermExpr::constant(&[IRI.to_owned(), c.class.clone(), String::new(), String::new()]);
            out.push((rdf_type.clone(), self.edges(&c.table, &c.subject, object, false, None)));
        }
        for p in &self.spec.properties {
            let object = p.object.term(&self.dialect, "t");
            out.push((
                p.predicate.clone(),
                self.edges(&p.table, &p.subject, object, true, Some(&p.object)),
            ));
        }
        let sub_of = RdfsVocab::rdfs_subclass_of_str().as_str().to_owned();
        for s in &self.spec.sub_class_of {
            let sub = TermExpr::constant(&[IRI.to_owned(), s.sub.clone(), String::new(), String::new()]);
            let sup = TermExpr::constant(&[IRI.to_owned(), s.sup.clone(), String::new(), String::new()]);
            let mut items = sub.items("s");
            items.extend(sup.items("o"));
            out.push((sub_of.clone(), SelectBuilder::new(items)));
        }
        out
    }
}

impl<D: SqlDialect> RelationalMapping for Tables<D> {
    fn class_extent(&self, class: &IriS) -> Option<Relation> {
        let classes = self.subclasses(class.as_str());
        let bodies: Vec<_> = self
            .spec
            .classes
            .iter()
            .filter(|c| classes.contains(&c.class))
            .map(|c| {
                SelectBuilder::new(c.subject.term(&self.dialect, "t").items("n"))
                    .from(table(&c.table, "t"))
                    .filter(self.not_null("t", &c.subject))
                    .into_set_expr()
            })
            .collect();
        union_all_of(bodies, false).map(|body| Relation(query(body)))
    }

    fn predicate(&self, predicate: &IriS) -> Option<PredicateRel> {
        let bodies: Vec<_> = self
            .all_edges()
            .into_iter()
            .filter(|(p, _)| p == predicate.as_str())
            .map(|(_, select)| select.into_set_expr())
            .collect();
        union_all_of(bodies, true).map(|body| PredicateRel(query(body)))
    }

    fn triples(&self) -> Query {
        let bodies: Vec<_> = self
            .all_edges()
            .into_iter()
            .map(|(p, select)| {
                let inner = select.into_query();
                let mut items = TermExpr::columns("e", "s").items("s");
                items.push(item(string(&p), PREDICATE_COLUMN));
                items.extend(TermExpr::columns("e", "o").items("o"));
                SelectBuilder::new(items).from(derived(inner, "e")).into_set_expr()
            })
            .collect();
        match union_all_of(bodies, true) {
            Some(body) => query(body),
            None => {
                // No triples at all: an empty relation with the right columns.
                let mut items = TermExpr::constant(&EncodedTerm::default()).items("s");
                items.push(item(string(""), PREDICATE_COLUMN));
                items.extend(TermExpr::constant(&EncodedTerm::default()).items("o"));
                SelectBuilder::new(items).filter(boolean(false)).into_query()
            },
        }
    }
}
