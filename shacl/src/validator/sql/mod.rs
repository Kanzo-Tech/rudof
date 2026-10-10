//! The SQL interpretation of the [algebra](crate::algebra): the plan of a
//! shapes graph rendered as a script, a temporary table per relation the
//! checks share and then one query, a `UNION ALL` of a `SELECT` per shape,
//! constraint component and context, run on the host's engine over one
//! `(s, p, o)` relation. The in-memory evaluator ([`crate::validator::eval`])
//! reads the same plan, and the W3C suite holds the two reports equal.
//!
//! ```text
//! validate(&IRSchema, triples: &str, focus: Option<&str>, &impl SqlEngine).await -> ValidationReport
//! fragment(&IRSchema, triples: &str, focus: Option<&str>, into: &str, &impl SqlEngine).await -> Vec<Unchecked>
//! ```
//!
//! - `triples` names the relation the data is read from, columns
//!   `s_type, s_value, p, o_type, o_value, o_datatype, o_lang` ([`triples`](self) has them). A table
//!   or a view: what tables lie under it is the host's, so the engine knows no
//!   mapping language and no product vocabulary.
//! - `focus`, when given, names a relation of nodes in `s_type, s_value`, spelled as
//!   the triples' subjects: each shape's focus nodes are then its targets
//!   among them, and the paths still read all of `triples`. A selection
//!   scopes validation without hiding the data it depends on.
//! - [`SqlEngine`] is implemented by the host: rudof links no engine. A
//!   DuckDB one, [`DuckDbEngine`], exists behind the native-only `duckdb`
//!   feature, for the tests.
//!
//! What compiles is what the algebra denotes: the engine's profile
//! ([`crate::algebra::profile`]). A shape outside it is not checked, and the
//! report lists it; every other shape is.
//!
//! The module builds for wasm: it depends on no engine, thread or I/O.

mod ast;
mod dialect;
#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
mod duckdb_host;
mod plan;
mod render;
mod term;
mod triples;

#[cfg(all(feature = "duckdb", not(target_family = "wasm")))]
pub use duckdb_host::{DuckDbEngine, DuckDbLoadError};
pub use plan::{Row, SqlEngine, SqlError};
pub use render::RESULT_COLUMNS;

use crate::algebra::{self, DenoteError, Plan, Unchecked, denote};
use crate::ir::IRSchema;
use crate::validator::report::ValidationReport;
use dialect::{Dialect, DuckDb};
use plan::SqlPlan;
use render::Renderer;
use sqlparser::ast::helpers::stmt_create_table::CreateTableBuilder;
use sqlparser::ast::{Ident, ObjectName, ObjectType, Query, Statement};
use triples::{Triples, relation};

/// Why a shapes graph does not compile to SQL.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SqlCompileError {
    /// What the SQL dialect cannot spell.
    #[error("the SQL engine does not compile {0}")]
    Unsupported(String),
    #[error("malformed target: {0}")]
    MalformedTarget(String),
    /// A relation's name is not a SQL object name.
    #[error("the relation {0}")]
    Relation(String),
    #[error("invalid data: {0}")]
    Data(String),
    #[error("internal error of the SQL compiler: {0}")]
    Internal(String),
}

impl From<DenoteError> for SqlCompileError {
    fn from(e: DenoteError) -> Self {
        match e {
            DenoteError::MalformedTarget(m) => SqlCompileError::MalformedTarget(m),
            DenoteError::Internal(m) => SqlCompileError::Internal(m),
        }
    }
}

/// Validates the data in the relation `triples` against `schema` on
/// `engine`: the script of the plan, run on one connection, and the report of
/// its rows, worded as the in-memory evaluator words its own.
///
/// A shape with targets yields the checks of its components and of its
/// property shapes; a shape without targets is reached only through another
/// (as a property shape, or by `sh:node`, `sh:and`, …). Deactivated shapes
/// yield nothing and conform everywhere.
pub async fn validate<E: SqlEngine>(
    schema: &IRSchema,
    triples: &str,
    focus: Option<&str>,
    engine: &E,
) -> Result<ValidationReport, SqlError<E::Error>> {
    compile(schema, &Triples::new(triples, focus)?, &DuckDb)?
        .run(engine)
        .await
}

fn compile<D: Dialect>(schema: &IRSchema, triples: &Triples, dialect: &D) -> Result<SqlPlan, SqlCompileError> {
    let plan = denote(schema, triples.scope().is_some())?;
    let roots: Vec<_> = plan.checks.iter().map(|c| c.rows).collect();
    let mut renderer = Renderer::new(&plan, triples, dialect);
    let tables = renderer.tables(&roots)?;
    let query = Statement::Query(Box::new(renderer.checks(&plan.checks)?));
    Ok(script(schema, plan, &tables, &query, dialect))
}

/// Writes the Shape Fragment of the data in the relation `triples` under
/// `schema` ([`crate::algebra::fragment`]) to the table `into`, replacing it:
/// the rows of `triples`, in its seven columns, whose triple makes a
/// conforming focus node conform. `focus` restricts the focus nodes as
/// [`validate`] does. Resolves to the shapes that have no fragment, because
/// they are outside the fragments profile.
pub async fn fragment<E: SqlEngine>(
    schema: &IRSchema,
    triples: &str,
    focus: Option<&str>,
    into: &str,
    engine: &E,
) -> Result<Vec<Unchecked>, SqlError<E::Error>> {
    compile_fragment(schema, &Triples::new(triples, focus)?, into, &DuckDb)?
        .create(engine)
        .await
}

fn compile_fragment<D: Dialect>(
    schema: &IRSchema,
    triples: &Triples,
    into: &str,
    dialect: &D,
) -> Result<SqlPlan, SqlCompileError> {
    let plan = algebra::fragment(schema, triples.scope().is_some())?;
    let root = plan
        .fragment
        .ok_or_else(|| SqlCompileError::Internal("a plan without a fragment".to_owned()))?;
    let mut renderer = Renderer::new(&plan, triples, dialect);
    let tables = renderer.tables(&[root])?;
    let create = CreateTableBuilder::new(ast::delimited(&relation(into)?))
        .or_replace(true)
        .query(Some(Box::new(renderer.fragment(root)?)))
        .build();
    Ok(script(schema, plan, &tables, &Statement::CreateTable(create), dialect))
}

/// The script of `plan`: its tables, then `statement`, then their teardown.
fn script<D: Dialect>(
    schema: &IRSchema,
    mut plan: Plan,
    tables: &[(String, Query)],
    statement: &Statement,
    dialect: &D,
) -> SqlPlan {
    let table = |name: &str| ObjectName::from(vec![Ident::with_quote('"', name)]);
    SqlPlan {
        setup: tables
            .iter()
            .map(|(name, body)| {
                let create = CreateTableBuilder::new(table(name))
                    .temporary(true)
                    .query(Some(Box::new(body.clone())))
                    .build();
                dialect.render(&Statement::CreateTable(create))
            })
            .collect(),
        query: dialect.render(statement),
        teardown: tables
            .iter()
            .rev()
            .map(|(name, _)| {
                dialect.render(&Statement::Drop {
                    object_type: ObjectType::Table,
                    if_exists: true,
                    names: vec![table(name)],
                    cascade: false,
                    restrict: false,
                    purge: false,
                    temporary: false,
                    table: None,
                })
            })
            .collect(),
        checks: std::mem::take(&mut plan.checks),
        unchecked: std::mem::take(&mut plan.unchecked),
        schema: schema.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudof_rdf::RDFFormat;
    use rudof_rdf::backend::ReaderMode;

    /// A shapes graph of many shapes, named and blank, most parameters given
    /// more than one value, as the order of a set would scramble them.
    fn shapes() -> String {
        let mut ttl = String::from(
            "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
             @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
             @prefix ex: <http://example.org/> .\n",
        );
        for i in 0..24 {
            let j = (i + 7) % 24;
            ttl.push_str(&format!(
                r#"
ex:S{i} a sh:NodeShape ;
  sh:targetClass ex:C{i}, ex:D{i} ;
  sh:targetSubjectsOf ex:p{i}, ex:q{i} ;
  sh:class ex:K{i}, ex:L{i} ;
  sh:node ex:T{i}, ex:T{j} ;
  sh:property [
    sh:path ex:p{i} ;
    sh:minCount 1 ; sh:maxCount 1 ;
    sh:datatype xsd:string ;
    sh:pattern "^[a-z]+$" ;
    sh:in ( "a" "b" "c" ) ;
  ] ;
  sh:property [ sh:path ex:p{i} ; sh:minLength 2 ; sh:class ex:K{j}, ex:L{j} ] ;
  sh:property [ sh:path ( ex:q{i} [ sh:inversePath ex:r{i} ] ) ; sh:node ex:T{j}, [ sh:class ex:M{i} ] ] ;
  sh:property [
    sh:path ex:r{i} ;
    sh:qualifiedValueShape [ sh:class ex:K{i} ] ; sh:qualifiedMinCount 1 ;
    sh:qualifiedValueShapesDisjoint true ;
  ] ;
  sh:property [
    sh:path ex:r{i} ;
    sh:qualifiedValueShape [ sh:class ex:L{i} ] ; sh:qualifiedMaxCount 2 ;
    sh:qualifiedValueShapesDisjoint true ;
  ] ;
  sh:or ( [ sh:datatype xsd:integer ] [ sh:nodeKind sh:IRI ] ) ;
  sh:closed true ; sh:ignoredProperties ( ex:x{i} ex:y{i} ex:z{i} ) .

ex:T{i} a sh:NodeShape ; sh:class ex:K{i} ; sh:property [ sh:path ex:t{i} ; sh:maxCount 3 ] .
"#
            ));
        }
        ttl
    }

    /// The setup, the query and the teardown `shapes` compile to, its own
    /// parse and its own maps; the script's number, which names its tables
    /// apart from another's, left out.
    fn script(shapes: &str) -> Vec<String> {
        let schema = IRSchema::from_str(shapes, &RDFFormat::Turtle, None, &ReaderMode::Strict).expect("shapes compile");
        let plan = compile(&schema, &Triples::new("triples", None).unwrap(), &DuckDb).expect("a plan");
        assert!(plan.unchecked.is_empty(), "{:?}", plan.unchecked);
        let run = plan
            .setup
            .first()
            .and_then(|s| s.split("\"shacl_").nth(1))
            .map(|s| s.split('_').next().unwrap_or_default().to_owned())
            .expect("a table");
        let tables = format!("\"shacl_{run}_");
        plan.setup
            .iter()
            .chain([&plan.query])
            .chain(&plan.teardown)
            .map(|s| s.replace(&tables, "\"shacl_"))
            .collect()
    }

    #[test]
    fn one_shapes_graph_compiles_to_one_script() {
        let shapes = shapes();
        let first = script(&shapes);
        assert!(first.len() > 3, "{} statements", first.len());
        for _ in 0..8 {
            assert!(script(&shapes) == first, "another script for the same shapes graph");
        }
    }
}
