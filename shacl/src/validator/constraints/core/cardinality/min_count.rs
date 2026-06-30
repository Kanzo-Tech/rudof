use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::FocusNodeIteration;
use crate::validator::nodes::FocusNodes;
use rudof_rdf::NeighsRDF;
use std::fmt::Debug;

/// `sh:minCount` — at least N value nodes are required.
pub(crate) struct MinCount(pub isize);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for MinCount {
    type Strategy = FocusNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        FocusNodeIteration
    }

    fn short_circuit(&self) -> bool {
        self.0 == 0
    }

    fn check<E: Engine<S>>(&self, vs: &FocusNodes<S>, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        Ok(if vs.len() < self.0 as usize {
            Check::Violate
        } else {
            Check::Hold
        })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!("MinCount({}) not satisfied", self.0)
    }
}
