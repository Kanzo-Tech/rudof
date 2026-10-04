use crate::{
    Result, Rudof,
    api::shacl::ShaclOperations,
    formats::{SqlDialectFormat, SqlMapping},
};
use shacl::validator::sql::SqlPlan;

/// Builder for the `compile_sql` operation.
///
/// Compiles the loaded SHACL shapes into a [`SqlPlan`]: one SQL query per
/// shape, constraint component and context, which a host runs on its own
/// engine; `SqlPlan::report` turns the rows back into a validation report.
pub struct CompileSqlBuilder<'a> {
    rudof: &'a Rudof,
    mapping: &'a SqlMapping,
    dialect: Option<&'a SqlDialectFormat>,
}

impl<'a> CompileSqlBuilder<'a> {
    /// Creates a new builder instance.
    ///
    /// This is called internally by `Rudof::compile_sql()` and should not
    /// be constructed directly.
    pub(crate) fn new(rudof: &'a Rudof, mapping: &'a SqlMapping) -> Self {
        Self {
            rudof,
            mapping,
            dialect: None,
        }
    }

    /// Sets the SQL dialect (DuckDB by default).
    pub fn with_dialect(mut self, dialect: &'a SqlDialectFormat) -> Self {
        self.dialect = Some(dialect);
        self
    }

    /// Compiles the shapes.
    pub fn execute(self) -> Result<SqlPlan> {
        <Rudof as ShaclOperations>::compile_sql(self.rudof, self.mapping, self.dialect)
    }
}
