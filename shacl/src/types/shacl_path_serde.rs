//! Local serde adapter giving `Deserialize` for [`SHACLPath`].
//!
//! `SHACLPath` lives in `rudof_rdf` and only derives `Serialize` (see
//! `rudof_rdf/src/path/shacl_path.rs`). Rather than reach across the crate
//! boundary, we recover the missing `Deserialize` direction locally with a
//! shadow enum whose wire shape is identical to `SHACLPath`'s derived
//! `Serialize` (same variant names, same field names, same leaf types). This is
//! the canonical serde pattern for a foreign type we cannot edit.
//!
//! Used through `#[serde(deserialize_with = "...")]` on the `path` field; the
//! serialize direction keeps `SHACLPath`'s own derive.

use rudof_iri::IriS;
use rudof_rdf::SHACLPath;
use serde::{Deserialize, Deserializer};

/// Mirror of [`SHACLPath`] that derives `Deserialize`. Kept private; converted
/// into the real type immediately after parsing.
#[derive(Deserialize)]
enum PathDef {
    Predicate { pred: IriS },
    Alternative { paths: Vec<PathDef> },
    Sequence { paths: Vec<PathDef> },
    Inverse { path: Box<PathDef> },
    ZeroOrMore { path: Box<PathDef> },
    OneOrMore { path: Box<PathDef> },
    ZeroOrOne { path: Box<PathDef> },
}

impl From<PathDef> for SHACLPath {
    fn from(def: PathDef) -> Self {
        match def {
            PathDef::Predicate { pred } => SHACLPath::Predicate { pred },
            PathDef::Alternative { paths } => SHACLPath::Alternative {
                paths: paths.into_iter().map(SHACLPath::from).collect(),
            },
            PathDef::Sequence { paths } => SHACLPath::Sequence {
                paths: paths.into_iter().map(SHACLPath::from).collect(),
            },
            PathDef::Inverse { path } => SHACLPath::Inverse {
                path: Box::new(SHACLPath::from(*path)),
            },
            PathDef::ZeroOrMore { path } => SHACLPath::ZeroOrMore {
                path: Box::new(SHACLPath::from(*path)),
            },
            PathDef::OneOrMore { path } => SHACLPath::OneOrMore {
                path: Box::new(SHACLPath::from(*path)),
            },
            PathDef::ZeroOrOne { path } => SHACLPath::ZeroOrOne {
                path: Box::new(SHACLPath::from(*path)),
            },
        }
    }
}

/// `deserialize_with` target for `SHACLPath`-typed fields.
pub fn deserialize<'de, D>(deserializer: D) -> Result<SHACLPath, D::Error>
where
    D: Deserializer<'de>,
{
    PathDef::deserialize(deserializer).map(SHACLPath::from)
}
