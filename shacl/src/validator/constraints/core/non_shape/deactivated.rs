use crate::validator::constraints::ConstraintComponent;
use crate::validator::iteration::ValueNodeIteration;
use rudof_rdf::NeighsRDF;
use std::fmt::Debug;

/// `sh:deactivated` — a deactivated shape raises no results. If the shape were
/// truly deactivated this component would never be reached; if it is active,
/// `sh:deactivated` itself never produces a violation. So the template
/// short-circuits to an empty result set.
pub(crate) struct Deactivated;

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Deactivated {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn short_circuit(&self) -> bool {
        true
    }
}
