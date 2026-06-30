use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use rudof_iri::IriS;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::literal::{ConcreteLiteral, Literal};
use std::fmt::Debug;

/// `sh:datatype` — each value node is a literal of the given datatype.
pub(crate) struct Datatype<'a>(pub &'a IriS);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for Datatype<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let Ok(lit) = S::term_as_literal(vn) else {
            return Ok(Check::Violate);
        };
        let violates = match TryInto::<ConcreteLiteral>::try_into(lit.clone()) {
            Ok(ConcreteLiteral::WrongDatatypeLiteral { .. }) | Err(_) => true,
            Ok(_) => lit
                .datatype()
                .get_iri()
                .map(|i| i.as_str() != self.0.as_str())
                .unwrap_or(true),
        };
        Ok(if violates { Check::Violate } else { Check::Hold })
    }

    fn message(&self, schema: &IRSchema) -> String {
        format!("Expected Datatype: {}", schema.prefix_map().qualify(self.0))
    }
}
