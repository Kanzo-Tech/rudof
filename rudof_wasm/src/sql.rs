//! The SQL engine across the ABI: the page's engine, `{ query(sql, { signal })
//! => Promise<Table> }`, driven from Rust. `Table` is `@fossil-lang/types`'
//! answer in columns (`numRows`, `getChild(name).get(i)`), which an Arrow
//! table is and `@kanzo-tech/mosaic`'s `engine()` answers. Validation is the
//! façade's (`FormEngine::validate_sql`); here the answers are only read back
//! as rows.

use js_sys::{Function, Object, Promise, Reflect};
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
    /// `engine` must have a `query` method, and `signal` is the
    /// `AbortSignal` every statement carries, as `query` takes one.
    pub fn new(engine: JsValue, signal: JsValue) -> Result<Self, String> {
        let query = Reflect::get(&engine, &"query".into())
            .ok()
            .and_then(|q| q.dyn_into::<Function>().ok())
            .ok_or("engine: expected an object with a query(sql, { signal }) method")?;
        if signal.is_undefined() || signal.is_null() {
            return Err("signal: expected the AbortSignal every statement carries".into());
        }
        Ok(Self { engine, query, signal })
    }

    async fn answer(&self, sql: &str) -> Result<JsValue, String> {
        let options = Object::new();
        Reflect::set(&options, &"signal".into(), &self.signal).map_err(message)?;
        let pending = self.query.call2(&self.engine, &sql.into(), &options).map_err(message)?;
        JsFuture::from(Promise::resolve(&pending)).await.map_err(message)
    }
}

impl SqlEngine for JsEngine {
    type Error = String;

    async fn execute(&self, sql: &str) -> Result<(), String> {
        self.answer(sql).await.map(drop)
    }

    /// The rows of the answer, a `Table`, read column by column: each of
    /// `RESULT_COLUMNS` through `getChild`, a column the answer lacks reading
    /// as `NULL`.
    async fn rows(&self, sql: &str) -> Result<Vec<SqlRow>, String> {
        let table = self.answer(sql).await?;
        let not_a_table = || "the engine's answer is not a table: it has no numRows and getChild(name)";
        let rows = Reflect::get(&table, &"numRows".into())
            .ok()
            .and_then(|n| n.as_f64())
            .ok_or_else(not_a_table)?;
        let get_child = Reflect::get(&table, &"getChild".into())
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
            .ok_or_else(not_a_table)?;
        let columns = RESULT_COLUMNS
            .iter()
            .map(|name| {
                let column = get_child.call1(&table, &(*name).into()).map_err(message)?;
                if column.is_null() || column.is_undefined() {
                    return Ok(None);
                }
                let get = Reflect::get(&column, &"get".into())
                    .ok()
                    .and_then(|f| f.dyn_into::<Function>().ok())
                    .ok_or_else(|| format!("the answer's column {name} has no get(index)"))?;
                Ok(Some((column, get)))
            })
            .collect::<Result<Vec<_>, String>>()?;
        (0..rows as u32)
            .map(|at| {
                columns
                    .iter()
                    .map(|column| match column {
                        None => Ok(None),
                        Some((column, get)) => get.call1(column, &at.into()).map(text).map_err(message),
                    })
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
    /// literal of objects, turned into a `Table` of the columns they name)
    /// and every other statement with nothing, and keeps each statement in
    /// `engine.seen`. It has no `toArray()`: the answer is read in columns.
    fn engine(rows: &str) -> JsValue {
        Function::new_no_args(&format!(
            "const seen = []; return {{ seen, query: async (sql, options) => {{ \
               seen.push(sql); \
               const rows = /^\\s*(CREATE|DROP)/i.test(sql) ? [] : {rows}; \
               const names = [...new Set(rows.flatMap(Object.keys))]; \
               return {{ numRows: rows.length, schema: {{ fields: names.map((name) => ({{ name }})) }}, \
                 getChild: (name) => names.includes(name) \
                   ? {{ length: rows.length, get: (i) => rows[i][name] ?? null }} : null }}; }} }};"
        ))
        .call0(&JsValue::NULL)
        .unwrap()
    }

    /// The options of a validation over `"job".triples` on `engine`, with a
    /// signal that never aborts.
    fn options(engine: &JsValue) -> js_sys::Object {
        let options = js_sys::Object::new();
        Reflect::set(&options, &"table".into(), &"\"job\".triples".into()).unwrap();
        Reflect::set(&options, &"engine".into(), engine).unwrap();
        let signal = Function::new_no_args("return new AbortController().signal")
            .call0(&JsValue::NULL)
            .unwrap();
        Reflect::set(&options, &"signal".into(), &signal).unwrap();
        options
    }

    async fn validate(shapes: &Shapes, engine: &JsValue) -> Result<JsValue, JsValue> {
        let options = options(engine);
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
    async fn a_focus_relation_scopes_the_targets() {
        let engine = engine("[]");
        let options = options(&engine);
        Reflect::set(&options, &"focus".into(), &"selection".into()).unwrap();
        JsFuture::from(shapes().validate(options.into()).unwrap())
            .await
            .expect("validates");
        let seen = js_sys::Array::from(&Reflect::get(&engine, &"seen".into()).unwrap());
        assert!(
            seen.iter().any(|sql| sql.as_string().unwrap().contains("selection")),
            "the script reads the focus relation"
        );
    }

    #[wasm_bindgen_test]
    async fn a_fragment_is_written_to_its_table() {
        let engine = engine("[]");
        let options = options(&engine);
        Reflect::set(&options, &"into".into(), &"\"job\".fragment".into()).unwrap();
        let fragment = JsFuture::from(shapes().fragment(options.into()).unwrap())
            .await
            .expect("a fragment");
        let unchecked = js_sys::Array::from(&Reflect::get(&fragment, &"unchecked".into()).unwrap());
        assert_eq!(unchecked.length(), 0);
        let seen = js_sys::Array::from(&Reflect::get(&engine, &"seen".into()).unwrap());
        assert!(
            seen.iter()
                .map(|sql| sql.as_string().unwrap())
                .any(|sql| sql.starts_with("CREATE OR REPLACE TABLE") && sql.contains("\"job\".\"fragment\"")),
            "the script writes the fragment: {seen:?}"
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
    async fn an_answer_of_rows_is_not_a_table() {
        let engine = Function::new_no_args("return { query: async () => ({ toArray: () => [] }) };")
            .call0(&JsValue::NULL)
            .unwrap();
        let error = validate(&shapes(), &engine)
            .await
            .expect_err("an answer without getChild");
        assert!(
            format!("{error:?}").contains("getChild"),
            "the error names the contract: {error:?}"
        );
    }

    #[wasm_bindgen_test]
    async fn a_validation_without_a_signal_is_refused() {
        let options = options(&engine("[]"));
        Reflect::delete_property(&options, &"signal".into()).unwrap();
        assert!(shapes().validate(options.into()).is_err());
    }

    #[wasm_bindgen_test]
    async fn an_object_without_query_is_not_an_engine() {
        assert!(validate(&shapes(), &JsValue::from(js_sys::Object::new()))
            .await
            .is_err());
    }
}
