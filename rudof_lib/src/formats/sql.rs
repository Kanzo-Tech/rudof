use crate::errors::ShaclError;
use shacl::validator::sql::TablesSpec;
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

/// Where the RDF terms live in tables, for the SHACL SQL engine.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SqlMapping {
    /// One `(s, p, o)` table holding arbitrary RDF, under this name (see
    /// `shacl::validator::sql::TripleTable`).
    TripleTable { table: String },
    /// Ordinary tables described by an R2RML-like mapping.
    Tables(TablesSpec),
}

impl SqlMapping {
    /// A [`SqlMapping::Tables`] read from its JSON form.
    pub fn tables_from_json(json: &str) -> Result<Self, ShaclError> {
        serde_json::from_str(json)
            .map(SqlMapping::Tables)
            .map_err(|e| ShaclError::InvalidSqlMapping { error: e.to_string() })
    }
}
