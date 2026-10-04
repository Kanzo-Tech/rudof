/// SQL dialects the SHACL SQL engine writes (`duckdb`, the default); the
/// table of dialects is shacl's.
pub use shacl::validator::sql::SqlDialectName as SqlDialectFormat;

/// Where the RDF terms live in tables, for the SHACL SQL engine: a triple
/// table, or ordinary tables described by an RML mapping (Turtle), with the
/// schema its unqualified table names resolve against.
pub use shacl::validator::sql::SqlMapping;
