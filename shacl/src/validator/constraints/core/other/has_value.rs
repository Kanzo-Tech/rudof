use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::FocusNodeIteration;
use crate::validator::nodes::FocusNodes;
use rudof_rdf::rdf_core::NeighsRDF;
use rudof_rdf::rdf_core::term::Object;
use std::fmt::Debug;

/// `sh:hasValue` — at least one value node equals the given term.
pub(crate) struct HasValue<'a>(pub &'a Object);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for HasValue<'_> {
    type Strategy = FocusNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        FocusNodeIteration
    }

    fn check<E: Engine<S>>(&self, vs: &FocusNodes<S>, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let value_term = S::object_as_term(self.0);
        Ok(if vs.iter().any(|v| v == &value_term) {
            Check::Hold
        } else {
            Check::Violate
        })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!("HasValue({}) not satisfied", self.0)
    }
}
