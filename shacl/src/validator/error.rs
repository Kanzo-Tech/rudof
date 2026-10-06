use rudof_rdf::RDFError;
use thiserror::Error;

/// Why a graph could not be validated, or a report not be read.
#[derive(Debug, Error)]
pub enum ValidationError {
    /// The shapes graph has no denotation (see [`crate::algebra::DenoteError`]).
    #[error(transparent)]
    Denote(#[from] crate::algebra::DenoteError),

    /// The plan could not be evaluated over the data graph.
    #[error(transparent)]
    Eval(#[from] crate::validator::eval::EvalError),

    /// The SQL host failed.
    #[error("SQL engine: {0}")]
    SqlEngine(String),

    /// Writing the report as RDF failed.
    #[error("report graph: {0}")]
    Graph(String),

    #[error(transparent)]
    RDFError(#[from] Box<RDFError>),

    #[error("Parsing error: the required field '{0}' is missing")]
    MissingRequiredField(String),

    #[error("Parsing error: the field '{field}' has an invalid IRI value: {value}")]
    InvalidIriValue { field: String, value: String },
}

impl From<RDFError> for ValidationError {
    fn from(value: RDFError) -> Self {
        Self::RDFError(Box::new(value))
    }
}
