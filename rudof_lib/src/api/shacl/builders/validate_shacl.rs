use crate::{Result, Rudof, api::shacl::ShaclOperations};

/// Builder for `validate_shacl` operation.
pub struct ValidateShaclBuilder<'a> {
    rudof: &'a mut Rudof,
}

impl<'a> ValidateShaclBuilder<'a> {
    /// Creates a new builder instance.
    ///
    /// This is called internally by `Rudof::validate_shacl()` and should not
    /// be constructed directly.
    pub(crate) fn new(rudof: &'a mut Rudof) -> Self {
        Self { rudof }
    }

    /// Executes the SHACL validation operation.
    pub fn execute(self) -> Result<()> {
        <Rudof as ShaclOperations>::validate_shacl(self.rudof)
    }
}
