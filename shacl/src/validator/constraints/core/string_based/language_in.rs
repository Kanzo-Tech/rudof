use crate::error::ValidationError;
use crate::ir::IRSchema;
use crate::validator::constraints::{Check, CheckCtx, ConstraintComponent};
use crate::validator::engine::Engine;
use crate::validator::iteration::ValueNodeIteration;
use rudof_rdf::NeighsRDF;
use rudof_rdf::term::literal::{Lang, Literal};
use std::fmt::Debug;

/// `sh:languageIn` — each literal value node uses one of the allowed languages.
pub(crate) struct LanguageIn<'a>(pub &'a [Lang]);

impl<S: NeighsRDF + Debug> ConstraintComponent<S> for LanguageIn<'_> {
    type Strategy = ValueNodeIteration;

    fn strategy(&self) -> Self::Strategy {
        ValueNodeIteration
    }

    fn check<E: Engine<S>>(&self, vn: &S::Term, _cx: &mut CheckCtx<'_, S, E>) -> Result<Check, ValidationError> {
        let violates = if let Ok(lit) = S::term_as_literal(vn) {
            match lit.lang() {
                None => true,
                Some(lang) => {
                    let lang_str = lang.to_string().to_lowercase();
                    !self.0.iter().any(|l| {
                        let l_str = l.to_string().to_lowercase();
                        lang_str == l_str || lang_str.starts_with(&format!("{}-", l_str))
                    })
                },
            }
        } else {
            true
        };
        Ok(if violates { Check::Violate } else { Check::Hold })
    }

    fn message(&self, _schema: &IRSchema) -> String {
        format!(
            "LanguageIn constraint not satisfied. Expected one of {}",
            self.0.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ")
        )
    }
}
