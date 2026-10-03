use crate::errors::ShaclError;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// SQL dialects the SHACL SQL engine writes.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum SqlDialectFormat {
    /// DuckDB (the default)
    #[default]
    DuckDb,
}

impl Display for SqlDialectFormat {
    fn fmt(&self, dest: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlDialectFormat::DuckDb => write!(dest, "duckdb"),
        }
    }
}

impl FromStr for SqlDialectFormat {
    type Err = ShaclError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "duckdb" => Ok(SqlDialectFormat::DuckDb),
            other => Err(ShaclError::UnsupportedSqlDialect {
                dialect: other.to_string(),
            }),
        }
    }
}

/// Where the RDF terms live in tables, for the SHACL SQL engine: a triple
/// table, or ordinary tables described by an RML mapping (Turtle), with the
/// schema its unqualified table names resolve against.
pub use shacl::validator::sql::SqlMapping;
