use rudof_rdf::rdf_core::RdfDataConfig;
use serde::{Deserialize, Serialize};
// File-based config loading is not available on wasm.
#[cfg(not(target_family = "wasm"))]
use crate::error::ShaclConfigError;
#[cfg(not(target_family = "wasm"))]
use std::fs::File;
#[cfg(not(target_family = "wasm"))]
use std::io::Read;
#[cfg(not(target_family = "wasm"))]
use std::path::Path;

/// This struct can be used to define the configuration of SHACLco
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct ShaclConfig {
    data: Option<RdfDataConfig>,
}

impl ShaclConfig {
    pub fn new() -> Self {
        Self {
            data: Some(RdfDataConfig::default()),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, ShaclConfigError> {
        let mut f = File::open(path)?;

        let mut s = String::new();
        f.read_to_string(&mut s)?;

        toml::from_str(s.as_str()).map_err(|e| ShaclConfigError::UnmarshallError(e.into()))
    }
}

impl Default for ShaclConfig {
    fn default() -> Self {
        Self::new()
    }
}
