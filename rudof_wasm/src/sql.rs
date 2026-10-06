//! The SQL engine across the ABI: the page's engine, `{ query(sql, { signal })
//! => Promise<Table> }` (`@kanzo-tech/mosaic`'s `engine()` as it is), driven
//! from Rust. Validation is the façade's (`FormEngine::validate_sql`); here
//! the engine's Arrow answers are only read back as rows.

use js_sys::{Array, Function, Object, Promise, Reflect};
use rudof_lib::form::{SqlEngine, SqlRow, RESULT_COLUMNS};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// The page's engine, and the signal every statement of one validation
/// carries.
pub struct JsEngine {
    engine: JsValue,
    query: Function,
    signal: JsValue,
}

fn message(e: JsValue) -> String {
    match e.dyn_ref::<js_sys::Error>() {
        Some(error) => String::from(error.message()),
        None => e.as_string().unwrap_or_else(|| format!("{e:?}")),
    }
}

/// A cell of an answer as text: SQL `NULL` is `None`, and a number (an
/// `INTEGER` column) or a `BigInt` (a `BIGINT` one) is written in decimal.
fn text(cell: JsValue) -> Option<String> {
    if cell.is_null() || cell.is_undefined() {
        None
    } else if let Some(s) = cell.as_string() {
        Some(s)
    } else if let Some(n) = cell.dyn_ref::<js_sys::BigInt>() {
        n.to_string(10).ok().map(String::from)
    } else {
        cell.as_f64().map(|n| n.to_string())
    }
}

impl JsEngine {
    /// `engine` must have a `query` method; `signal`, when not `undefined`,
    /// is an `AbortSignal` that stops the running statement.
    pub fn new(engine: JsValue, signal: JsValue) -> Result<Self, String> {
        let query = Reflect::get(&engine, &"query".into())
            .ok()
            .and_then(|q| q.dyn_into::<Function>().ok())
            .ok_or("engine: expected an object with a query(sql, { signal }) method")?;
        Ok(Self { engine, query, signal })
    }

    async fn answer(&self, sql: &str) -> Result<JsValue, String> {
        let options = Object::new();
        if !self.signal.is_undefined() {
            Reflect::set(&options, &"signal".into(), &self.signal).map_err(message)?;
        }
        let pending = self.query.call2(&self.engine, &sql.into(), &options).map_err(message)?;
        JsFuture::from(Promise::resolve(&pending)).await.map_err(message)
    }
}

impl SqlEngine for JsEngine {
    type Error = String;

    async fn execute(&self, sql: &str) -> Result<(), String> {
        self.answer(sql).await.map(drop)
    }

    /// The rows of the answer, an Arrow `Table`, by column name.
    async fn rows(&self, sql: &str) -> Result<Vec<SqlRow>, String> {
        let table = self.answer(sql).await?;
        let rows = Reflect::get(&table, &"toArray".into())
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
            .ok_or("the engine's answer is not a table: it has no toArray()")?
            .call0(&table)
            .map_err(message)?;
        Array::from(&rows)
            .iter()
            .map(|row| {
                RESULT_COLUMNS
                    .iter()
                    .map(|column| Reflect::get(&row, &(*column).into()).map(text).map_err(message))
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use js_sys::{Function, Reflect};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_futures::JsFuture;
    use wasm_bindgen_test::wasm_bindgen_test;

    use crate::Shapes;

    const SHAPES: &str = r#"@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix : <http://example.org/> .
:S a sh:NodeShape ; sh:targetClass :C ; sh:property [ sh:path :p ; sh:minCount 1 ] ."#;

    /// An engine that answers the script's query with `rows` (a JS array
    /// literal) and every other statement with nothing, and keeps each
    /// statement in `engine.seen`.
    fn engine(rows: &str) -> JsValue {
        Function::new_no_args(&format!(
            "const seen = []; return {{ seen, query: async (sql, options) => {{ \
               seen.push(sql); \
               const rows = /^\\s*(CREATE|DROP)/i.test(sql) ? [] : {rows}; \
               return {{ toArray: () => rows }}; }} }};"
        ))
        .call0(&JsValue::NULL)
        .unwrap()
    }

    async fn validate(shapes: &Shapes, engine: &JsValue) -> Result<JsValue, JsValue> {
        let options = js_sys::Object::new();
        Reflect::set(&options, &"table".into(), &"\"job\".triples".into()).unwrap();
        Reflect::set(&options, &"engine".into(), engine).unwrap();
        let promise = shapes.validate(options.into()).map_err(JsValue::from)?;
        JsFuture::from(promise).await
    }

    fn shapes() -> Shapes {
        Shapes::parse(SHAPES.to_owned(), None).unwrap()
    }

    #[wasm_bindgen_test]
    async fn the_engine_runs_the_script_over_the_table_and_its_rows_are_the_report() {
        let engine = engine(
            "[{ check: 0, focus_kind: 'I', focus_value: 'http://example.org/n', focus_datatype: '', \
               focus_lang: '', value_kind: null, value_value: null, value_datatype: null, value_lang: null, \
               path: null }]",
        );
        let report = validate(&shapes(), &engine).await.expect("validates");
        assert_eq!(Reflect::get(&report, &"conforms".into()).unwrap(), JsValue::FALSE);
        let results = js_sys::Array::from(&Reflect::get(&report, &"results".into()).unwrap());
        assert_eq!(results.length(), 1);
        let seen = js_sys::Array::from(&Reflect::get(&engine, &"seen".into()).unwrap());
        assert!(
            seen.iter()
                .any(|sql| sql.as_string().unwrap().contains("\"job\".\"triples\"")),
            "the script reads the table it was given"
        );
    }

    #[wasm_bindgen_test]
    async fn no_rows_conform() {
        let report = validate(&shapes(), &engine("[]")).await.expect("validates");
        assert_eq!(Reflect::get(&report, &"conforms".into()).unwrap(), JsValue::TRUE);
    }

    #[wasm_bindgen_test]
    async fn a_shape_outside_the_profile_is_listed_and_the_rest_validated() {
        let shapes = Shapes::parse(
            format!(
                "{SHAPES}\n:Q a sh:NodeShape ; sh:targetClass :C ; sh:sparql [ sh:select \"SELECT $this {{}}\" ] ."
            ),
            None,
        )
        .unwrap();
        let report = validate(&shapes, &engine("[]")).await.expect("validates");
        let unchecked = js_sys::Array::from(&Reflect::get(&report, &"unchecked".into()).unwrap());
        assert_eq!(unchecked.length(), 1);
        let reason = Reflect::get(&unchecked.get(0), &"reason".into()).unwrap();
        assert!(
            reason.as_string().unwrap().contains("SPARQLConstraintComponent"),
            "{reason:?}"
        );
    }

    #[wasm_bindgen_test]
    async fn a_row_naming_no_check_is_an_error() {
        let engine = engine(
            "[{ check: 7, focus_kind: 'I', focus_value: 'http://example.org/n', focus_datatype: '', \
               focus_lang: '' }]",
        );
        assert!(validate(&shapes(), &engine).await.is_err());
    }

    #[wasm_bindgen_test]
    async fn an_object_without_query_is_not_an_engine() {
        assert!(validate(&shapes(), &JsValue::from(js_sys::Object::new()))
            .await
            .is_err());
    }
}
