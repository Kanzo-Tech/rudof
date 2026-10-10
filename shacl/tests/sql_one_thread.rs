//! On one thread, as DuckDB-WASM runs, a validation whose rows stream back
//! returns. A `sh:minExclusive` check, whose comparison by value tests the
//! datatype against a list of constants, followed in the `UNION ALL` by a
//! `sh:class` check, whose `rdfs:subClassOf*` is a recursive CTE, deadlocked
//! DuckDB 1.5 while the list was spelled `IN (…)` (see `ast::one_of`).
#![cfg(not(target_family = "wasm"))]

use duckdb::Connection;
use duckdb::arrow::array::{Array, AsArray};
use duckdb::arrow::compute::cast;
use duckdb::arrow::datatypes::DataType;
use futures::executor::block_on;
use rudof_rdf::RDFFormat;
use rudof_rdf::backend::ReaderMode;
use shacl::ir::IRSchema;
use shacl::validator::sql::{Row, SqlEngine, validate};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
ex:ProjectShape a sh:NodeShape ;
    sh:targetClass ex:Project ;
    sh:property [ sh:path ex:cost ; sh:minExclusive 0 ] ;
    sh:property [ sh:path ex:scheme ; sh:class ex:Scheme ; sh:minCount 1 ] .
"#;

/// Two tables of costs, half of them 0, under a triples view as a host states
/// it: a branch per class and per column, a double spelled as its lexical form.
const DATA: &str = r#"
CREATE TABLE project AS
    SELECT 'http://example.org/project/' || (100000000 + i) AS subject,
           CASE WHEN i % 2 = 0 THEN 0 ELSE i * 13.5 END::DOUBLE AS cost
    FROM range(23451) r(i);
CREATE TABLE participation AS
    SELECT 'http://example.org/participation/' || i || '/partner' AS subject,
           CASE WHEN i % 2 = 0 THEN 0 ELSE i * 7.25 END::DOUBLE AS cost
    FROM range(145274) r(i);
CREATE VIEW triples AS
          SELECT 'I' AS s_type, subject AS s_value, 'http://www.w3.org/1999/02/22-rdf-syntax-ns#type' AS p,
                 'I' AS o_type, 'http://example.org/Project' AS o_value, '' AS o_datatype, '' AS o_lang
          FROM project
UNION ALL SELECT 'I', subject, 'http://example.org/cost', 'L',
                 CASE WHEN isnan(cost) THEN 'NaN' ELSE cost::VARCHAR END, 'http://www.w3.org/2001/XMLSchema#decimal', ''
          FROM project WHERE cost IS NOT NULL
UNION ALL SELECT 'I', subject, 'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'I', 'http://example.org/Participation', '', ''
          FROM participation
UNION ALL SELECT 'I', subject, 'http://example.org/cost', 'L',
                 CASE WHEN isnan(cost) THEN 'NaN' ELSE cost::VARCHAR END, 'http://www.w3.org/2001/XMLSchema#decimal', ''
          FROM participation WHERE cost IS NOT NULL;
"#;

/// An engine that streams the rows of a query, chunk by chunk, as
/// DuckDB-WASM answers one.
struct Streaming(Connection);

impl SqlEngine for Streaming {
    type Error = duckdb::Error;

    async fn execute(&self, sql: &str) -> Result<(), Self::Error> {
        self.0.execute_batch(sql)
    }

    async fn rows(&self, sql: &str) -> Result<Vec<Row>, Self::Error> {
        let mut statement = self.0.prepare(sql)?;
        let mut rows = Vec::new();
        for batch in statement.stream_arrow([])? {
            let columns: Vec<_> = batch
                .columns()
                .iter()
                .map(|c| cast(c, &DataType::Utf8).expect("a column as text"))
                .collect();
            for i in 0..batch.num_rows() {
                rows.push(
                    columns
                        .iter()
                        .map(|c| {
                            let c = c.as_string::<i32>();
                            (!c.is_null(i)).then(|| c.value(i).to_owned())
                        })
                        .collect(),
                );
            }
        }
        Ok(rows)
    }
}

#[test]
fn a_streamed_validation_returns_on_one_thread() {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let schema = IRSchema::from_str(SHAPES, &RDFFormat::Turtle, None, &ReaderMode::Strict).expect("shapes");
        let connection = Connection::open_in_memory().expect("duckdb opens");
        connection.execute_batch(DATA).expect("data loads");
        connection.execute_batch("SET threads = 1").expect("one thread");
        let engine = Streaming(connection);
        let report = block_on(validate(&schema, "triples", None, &engine)).expect("validates");
        done.send(report.results().len()).ok();
    });
    // A deadlock never returns: the timeout fails the test instead of blocking it.
    let results = finished
        .recv_timeout(Duration::from_secs(60))
        .expect("the validation returns on one thread");
    // Each project of cost 0, and each project without a scheme.
    assert_eq!(results, 11726 + 23451);
}
