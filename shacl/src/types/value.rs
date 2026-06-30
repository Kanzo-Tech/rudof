use prefixmap::IriRef;
use rudof_iri::IriS;
use rudof_rdf::term::literal::ConcreteLiteral;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Value {
    Iri(IriRef),
    Literal(ConcreteLiteral),
}

impl From<IriS> for Value {
    fn from(value: IriS) -> Self {
        Value::Iri(IriRef::iri(value))
    }
}

impl From<ConcreteLiteral> for Value {
    fn from(value: ConcreteLiteral) -> Self {
        Value::Literal(value)
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Iri(iri) => write!(f, "value({iri})"),
            Value::Literal(lit) => write!(f, "literal({lit})"),
        }
    }
}
