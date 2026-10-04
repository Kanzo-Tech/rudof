use crate::{
    Result, Rudof,
    errors::ShaclError,
    formats::{SqlDialectFormat, SqlMapping},
};
use shacl::validator::sql::SqlPlan;

/// Compiles the loaded SHACL shapes into the SQL checks that find their
/// validation results in the tables `mapping` describes.
pub fn compile_sql(rudof: &Rudof, mapping: &SqlMapping, dialect: Option<&SqlDialectFormat>) -> Result<SqlPlan> {
    let schema = rudof.shacl_shapes.as_ref().ok_or(ShaclError::NoShaclShapesLoaded)?;
    let plan = dialect
        .copied()
        .unwrap_or_default()
        .compile(mapping, schema)
        .map_err(|e| ShaclError::FailedCompilingSql { error: e.to_string() })?;
    Ok(plan)
}
