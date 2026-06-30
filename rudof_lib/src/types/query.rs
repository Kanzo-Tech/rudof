use rudof_rdf::query::QuerySolutions;
use sparql_service::RdfData;

#[derive(Debug, Clone)]
pub enum QueryResult {
    Select(QuerySolutions<RdfData>),
    Construct(String),
    Ask(bool),
}
