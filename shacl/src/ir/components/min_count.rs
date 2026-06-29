use crate::ir::error::IRError;
use std::fmt::{Display, Formatter};

/// sh:minCount specifies the minimum number of value nodes that satisfy the
/// condition. If the minimum cardinality value is 0 then this constraint is
/// always satisfied and so may be omitted.
///
/// - IRI: https://www.w3.org/TR/shacl/#MinCountConstraintComponent
/// - DEF: If the number of value nodes is less than $minCount, there is a
///   validation result.
#[derive(Debug, Clone)]
pub struct MinCount {
    min_count: usize,
}

impl MinCount {
    /// Validates `min_count >= 0` instead of silently wrapping a negative
    /// `isize` into a huge `usize`.
    pub fn new(min_count: isize) -> Result<Self, IRError> {
        let min_count = usize::try_from(min_count).map_err(|_| IRError::NegativeCardinality {
            component: "sh:minCount",
            value: min_count,
        })?;
        Ok(MinCount { min_count })
    }

    pub fn min_count(&self) -> usize {
        self.min_count
    }
}

impl Display for MinCount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "MinCount: {}", self.min_count())
    }
}
