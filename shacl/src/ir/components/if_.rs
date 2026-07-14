use crate::ir::shape_label_idx::ShapeLabelIdx;
use std::fmt::{Display, Formatter};

/// sh:if / sh:then / sh:else — the SHACL-AF conditional constraint. Each value
/// node is checked against the `cond` shape: if it conforms, it must conform to
/// `then` (when present); otherwise it must conform to `els` (when present). A
/// missing branch means "no constraint".
///
/// This is equivalent to `Or(And(cond, then), And(not cond, els))` but modelled
/// directly. Holds interned shape indices for the condition and the two optional
/// branches.
#[derive(Debug, Clone)]
pub struct If {
    cond: ShapeLabelIdx,
    then: Option<ShapeLabelIdx>,
    els: Option<ShapeLabelIdx>,
}

impl If {
    pub fn new(cond: ShapeLabelIdx, then: Option<ShapeLabelIdx>, els: Option<ShapeLabelIdx>) -> Self {
        If { cond, then, els }
    }

    pub fn cond(&self) -> &ShapeLabelIdx {
        &self.cond
    }

    pub fn then(&self) -> Option<&ShapeLabelIdx> {
        self.then.as_ref()
    }

    pub fn els(&self) -> Option<&ShapeLabelIdx> {
        self.els.as_ref()
    }
}

impl Display for If {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "If(cond: {}", self.cond)?;
        if let Some(then) = self.then {
            write!(f, ", then: {then}")?;
        }
        if let Some(els) = self.els {
            write!(f, ", else: {els}")?;
        }
        write!(f, ")")
    }
}
