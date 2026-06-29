use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::Object;
use std::fmt::Debug;

/// `sh:in` — each value node is a member of the given list.
pub(crate) struct In<'a>(pub &'a [Object]);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for In<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let values = self.0.iter().map(S::object_as_term).collect::<Vec<_>>();
        Ok(if values.contains(vn) { Check::Hold } else { Check::Violate })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!("In constraint not satisfied. Expected one of {:?}", self.0)
    }
}
