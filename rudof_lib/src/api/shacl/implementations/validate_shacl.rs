use crate::{
    Result, Rudof,
    errors::{DataError, ShaclError},
    types::Data,
};
use shacl::ir::IRSchema;

pub fn validate_shacl(rudof: &mut Rudof) -> Result<()> {
    let (data, shacl_schema_ir) = validate_loaded_data_schema_and_shapes(rudof)?;

    let result = shacl::validator::validate(shacl_schema_ir, data.unwrap_rdf_mut())
        .map_err(|e| ShaclError::FailedShaclValidation { error: e.to_string() })?;

    rudof.shacl_validation_results = Some(result);

    Ok(())
}

fn validate_loaded_data_schema_and_shapes(rudof: &mut Rudof) -> Result<(&mut Data, &IRSchema)> {
    let data = rudof.data.as_mut().ok_or(Box::new(DataError::NoDataLoaded))?;

    if !data.is_rdf() {
        Err(Box::new(DataError::NoRdfDataLoaded))?
    }

    let shacl_schema_ir = rudof.shacl_shapes.as_ref().ok_or(ShaclError::NoShaclShapesLoaded)?;

    Ok((data, shacl_schema_ir))
}
