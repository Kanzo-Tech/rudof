use crate::ir::error::IRError;
use std::fmt::{Display, Formatter};

/// sh:maxCount specifies the maximum number of value nodes that satisfy the
/// condition.
///
/// - IRI: https://www.w3.org/TR/shacl/#MaxCountConstraintComponent
/// - DEF: If the number of value nodes is greater than $maxCount, there is a
///   validation result.
#[derive(Debug, Clone)]
pub struct MaxCount {
    max_count: usize,
}

impl MaxCount {
    /// Validates `max_count >= 0` instead of silently wrapping a negative
    /// `isize` into a huge `usize`.
    pub fn new(max_count: isize) -> Result<Self, IRError> {
        let max_count = usize::try_from(max_count).map_err(|_| IRError::NegativeCardinality {
            component: "sh:maxCount",
            value: max_count,
        })?;
        Ok(MaxCount { max_count })
    }

    pub fn max_count(&self) -> usize {
        self.max_count
    }
}

impl Display for MaxCount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "MaxCount: {}", self.max_count())
    }
}
