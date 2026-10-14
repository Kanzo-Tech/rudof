//! `Mapping`: an RML-Core mapping across the ABI. It renders the `(s, p, o)` relation on the page's engine (the
//! relation `Shapes` validates), and says per table what the mapping means: the class, and per column the
//! predicate, the term type, the datatype, the language, and the annotations the mapping graph carries.

use crate::dto::{ParseOptions, TermValue};
use crate::sql::{self, message};
use crate::{format_of, from_js, shapes_error, string_option, to_js};
use js_sys::Function;
use rudof_lib::form::rml::ast::{Annotation, Constant, Expression, Iterable, LanguageMap, LiteralKind, Node, TermType};
use rudof_lib::form::rml::model::{Column, Model, Relation, Table};
use rudof_lib::form::SqlEngine;
use serde::{Deserialize, Serialize};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

/// A parsed RML-Core mapping.
#[wasm_bindgen]
pub struct Mapping {
    inner: rudof_lib::form::Mapping,
}

#[wasm_bindgen]
impl Mapping {
    /// Parse a mapping graph. `options.mediaType` defaults to Turtle; `options.base` is the document base relative
    /// IRIs resolve against, and the base IRI a generated relative IRI takes unless its triples map states its own
    /// (RML-Core §4.1.1; without one, `http://example.org/`). Throws a `ShapesError`: a syntax error is placed
    /// with `line` and `column`; a mapping that is not RML-Core this reads, or that uses what it does not model
    /// (graph maps, other reference formulations), is an error that names the component.
    pub fn parse(text: String, options: Option<Ts<ParseOptions>>) -> Result<Mapping, JsValue> {
        let ParseOptions { media_type, base } = options.map(from_js).transpose()?.unwrap_or_default();
        let inner = rudof_lib::form::Mapping::parse(
            &text,
            &format_of(media_type.as_deref().unwrap_or("text/turtle")),
            base.as_deref(),
        )
        .map_err(shapes_error)?;
        Ok(Mapping { inner })
    }

    /// Create, or replace, the view `into` as the mapping's triples: a relation of columns
    /// `s_type, s_value, p, o_type, o_value, o_datatype, o_lang`, which `Shapes.validate` reads. `tables` maps each
    /// table the mapping names to the SQL name of the relation that holds it (`(t) => '"job"."' + t + '"'`);
    /// `engine` is `{ query(sql, { signal }): Promise<Table> }` and `signal` stops the running statement.
    /// Resolves when the view exists. Nothing is read: the view is a query, run when it is.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn triples(
        &self,
        #[wasm_bindgen(unchecked_param_type = "MappingTriples")] options: JsValue,
    ) -> Result<js_sys::Promise, JsError> {
        let field = |name: &str| js_sys::Reflect::get(&options, &name.into()).unwrap_or(JsValue::UNDEFINED);
        let into = string_option(&options, "into", "the name of the view to create")?
            .ok_or_else(|| JsError::new("into: expected the name of the view to create"))?;
        let tables: Function = field("tables")
            .dyn_into()
            .map_err(|_| JsError::new("tables: expected a function (table) => the SQL name of its relation"))?;
        let engine = sql::JsEngine::new(field("engine"), field("signal")).map_err(|e| JsError::new(&e))?;
        let statement = self
            .inner
            .create_view(&into, |table| {
                let resolved = tables.call1(&JsValue::UNDEFINED, &table.into()).map_err(message)?;
                resolved
                    .as_string()
                    .ok_or_else(|| "tables returned something other than a string".to_owned())
            })
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            engine.execute(&statement).await.map_err(|e| JsError::new(&e))?;
            Ok(JsValue::UNDEFINED)
        }))
    }

    /// What the mapping says per table: see `MappingModel`.
    pub fn model(&self) -> Result<Ts<MappingModel>, JsError> {
        to_js(&model_of(&self.inner.model()))
    }
}

// ---- the model ----------------------------------------------------------------

/// What a mapping says about its tables, one entry per triples map.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MappingModel {
    pub tables: Vec<MappingTable>,
}

/// What one triples map says about the rows of its logical source.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MappingTable {
    /// The triples map: its IRI, or `_:label`.
    pub id: String,
    pub source: MappingSource,
    /// How a row is named.
    pub subject: MappingValue,
    /// The classes of a row's subject, as IRIs.
    pub classes: Vec<String>,
    /// The annotations about those classes: `subject` is the class.
    pub annotations: Vec<MappingAnnotation>,
    pub columns: Vec<MappingColumn>,
    pub relations: Vec<MappingRelation>,
}

/// The logical source: a table by the name the host resolves, or a query.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MappingSource {
    Table { name: String },
    Query { sql: String },
}

/// Where a term comes from: a column of the row, a template over columns, or a constant.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MappingValue {
    Column { name: String },
    Template { template: String },
    Constant { term: TermValue },
}

/// One predicate-object pair whose object is a value of the row.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MappingColumn {
    pub predicate: String,
    pub value: MappingValue,
    /// `IRI`, `UnsafeIRI`, `BlankNode` or `Literal`, the default of RML-Core §8.4.1 applied.
    pub term_type: String,
    /// The datatype the mapping states. Absent is the natural datatype of the column's SQL type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<MappingLanguage>,
    /// The annotations about the predicate: `subject` is the predicate.
    pub annotations: Vec<MappingAnnotation>,
}

/// The language of a literal: a constant tag, or the column that holds it.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MappingLanguage {
    Constant { tag: String },
    Column { name: String },
}

/// A property whose object is the subject of another triples map.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MappingRelation {
    pub predicate: String,
    /// The parent triples map: its IRI, or `_:label`.
    pub parent: String,
    pub annotations: Vec<MappingAnnotation>,
}

/// A triple of the mapping graph about an IRI the mapping names, such as `rdfs:label`.
#[derive(Tsify, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MappingAnnotation {
    /// The class or predicate it is about.
    pub subject: String,
    pub predicate: String,
    pub value: TermValue,
}

fn node(n: &Node) -> String {
    match n {
        Node::Iri(iri) => iri.as_str().to_owned(),
        Node::Blank(label) => format!("_:{label}"),
    }
}

fn term(c: &Constant) -> TermValue {
    match c {
        Constant::Iri(iri) => TermValue::named(iri.as_str()),
        Constant::Literal(l) => match &l.kind {
            LiteralKind::Datatype(d) => TermValue::literal(&l.lexical_form, Some(d.as_str().to_owned()), None),
            LiteralKind::Language(tag) => TermValue::literal(&l.lexical_form, None, Some(tag.clone())),
        },
    }
}

fn value(e: &Expression) -> MappingValue {
    match e {
        Expression::Reference(name) => MappingValue::Column { name: name.clone() },
        Expression::Template(t) => MappingValue::Template {
            template: t.to_string(),
        },
        Expression::Constant(c) => MappingValue::Constant { term: term(c) },
    }
}

fn annotation(a: &Annotation) -> MappingAnnotation {
    MappingAnnotation {
        subject: a.subject.as_str().to_owned(),
        predicate: a.predicate.as_str().to_owned(),
        value: term(&a.object),
    }
}

fn column(c: &Column) -> MappingColumn {
    MappingColumn {
        predicate: c.predicate.as_str().to_owned(),
        value: value(&c.value),
        term_type: match c.term_type {
            TermType::Iri => "IRI",
            TermType::UnsafeIri => "UnsafeIRI",
            TermType::BlankNode => "BlankNode",
            TermType::Literal => "Literal",
        }
        .to_owned(),
        datatype: c.datatype.as_ref().map(|d| d.as_str().to_owned()),
        language: c.language.as_ref().map(|l| match l {
            LanguageMap::Constant(tag) => MappingLanguage::Constant { tag: tag.clone() },
            LanguageMap::Reference(name) => MappingLanguage::Column { name: name.clone() },
        }),
        annotations: c.annotations.iter().map(annotation).collect(),
    }
}

fn relation(r: &Relation) -> MappingRelation {
    MappingRelation {
        predicate: r.predicate.as_str().to_owned(),
        parent: node(&r.parent),
        annotations: r.annotations.iter().map(annotation).collect(),
    }
}

fn table(t: &Table) -> MappingTable {
    MappingTable {
        id: node(&t.triples_map),
        source: match &t.source {
            Iterable::Sql2008Table(name) => MappingSource::Table { name: name.clone() },
            Iterable::Sql2008Query(sql) => MappingSource::Query { sql: sql.clone() },
        },
        subject: value(&t.subject),
        classes: t.classes.iter().map(|c| c.as_str().to_owned()).collect(),
        annotations: t.annotations.iter().map(annotation).collect(),
        columns: t.columns.iter().map(column).collect(),
        relations: t.relations.iter().map(relation).collect(),
    }
}

pub(crate) fn model_of(model: &Model) -> MappingModel {
    MappingModel {
        tables: model.tables.iter().map(table).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use js_sys::Reflect;
    use wasm_bindgen_futures::JsFuture;
    use wasm_bindgen_test::wasm_bindgen_test;

    const MAPPING: &str = r#"@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <https://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
<#Person> rml:logicalSource [ rml:source [ a rml:Source ] ; rml:referenceFormulation rml:SQL2008Table ; rml:iterator "Person" ] ;
  rml:subjectMap [ rml:reference "subject" ; rml:termType rml:IRI ; rml:class ex:Person ] ;
  rml:predicateObjectMap [ rml:predicate ex:name ; rml:objectMap [ rml:reference "name" ; rml:datatype xsd:string ] ] ,
                         [ rml:predicate ex:knows ; rml:objectMap [ rml:parentTriplesMap <#Person> ] ] .
<#Nickname> rml:logicalSource [ rml:source [ a rml:Source ] ; rml:referenceFormulation rml:SQL2008Query ;
    rml:iterator "SELECT subject, n.value AS value, n.lang AS lang FROM (SELECT subject, unnest(nickname) AS n FROM Person)" ] ;
  rml:subjectMap [ rml:reference "subject" ; rml:termType rml:IRI ] ;
  rml:predicateObjectMap [ rml:predicate ex:nickname ; rml:objectMap [ rml:reference "value" ; rml:languageMap [ rml:reference "lang" ] ] ] .
ex:Person rdfs:label "Person"@en, "Persona"@es .
ex:name rdfs:label "name"@en .
"#;

    fn mapping() -> Mapping {
        Mapping::parse(MAPPING.to_owned(), None).expect("parses")
    }

    fn model() -> MappingModel {
        from_js(mapping().model().expect("a model")).expect("reads back")
    }

    /// An engine that keeps every statement in `seen` and answers each with an empty table.
    fn engine() -> JsValue {
        Function::new_no_args(
            "const seen = []; return { seen, query: async (sql, options) => { seen.push(sql); \
             return { numRows: 0, getChild: () => null }; } };",
        )
        .call0(&JsValue::NULL)
        .unwrap()
    }

    fn options(engine: &JsValue, tables: &str) -> js_sys::Object {
        let options = js_sys::Object::new();
        let set = |k: &str, v: &JsValue| Reflect::set(&options, &k.into(), v).unwrap();
        set("into", &"\"job\".triples".into());
        set("engine", engine);
        set("tables", &Function::new_with_args("t", tables));
        set(
            "signal",
            &Function::new_no_args("return new AbortController().signal")
                .call0(&JsValue::NULL)
                .unwrap(),
        );
        options
    }

    fn message(error: JsValue) -> String {
        js_sys::Error::from(error).message().as_string().unwrap()
    }

    fn seen(engine: &JsValue) -> Vec<String> {
        js_sys::Array::from(&Reflect::get(engine, &"seen".into()).unwrap())
            .iter()
            .map(|s| s.as_string().unwrap())
            .collect()
    }

    #[wasm_bindgen_test]
    fn the_model_says_what_each_table_means() {
        let model = model();
        assert_eq!(model.tables.len(), 2);
        let person = model
            .tables
            .iter()
            .find(|t| t.source == MappingSource::Table { name: "Person".into() })
            .unwrap();
        assert_eq!(person.classes, vec!["https://example.org/Person".to_owned()]);
        assert_eq!(person.subject, MappingValue::Column { name: "subject".into() });
        let labels: Vec<_> = person
            .annotations
            .iter()
            .map(|a| (a.value.value.as_str(), a.value.language.as_deref()))
            .collect();
        assert!(
            labels.contains(&("Person", Some("en"))) && labels.contains(&("Persona", Some("es"))),
            "{labels:?}"
        );
        let name = person
            .columns
            .iter()
            .find(|c| c.predicate == "https://example.org/name")
            .unwrap();
        assert_eq!(name.value, MappingValue::Column { name: "name".into() });
        assert_eq!(name.term_type, "Literal");
        assert_eq!(
            name.datatype.as_deref(),
            Some("http://www.w3.org/2001/XMLSchema#string")
        );
        assert_eq!(name.annotations.len(), 1, "the label of the predicate");
        assert_eq!(person.relations.len(), 1);
        assert!(person.relations[0].parent.ends_with("#Person"));
    }

    #[wasm_bindgen_test]
    fn a_language_tagged_column_names_the_column_that_holds_the_tag() {
        let model = model();
        let nickname = model
            .tables
            .iter()
            .find(|t| matches!(t.source, MappingSource::Query { .. }))
            .unwrap();
        let column = &nickname.columns[0];
        assert_eq!(column.value, MappingValue::Column { name: "value".into() });
        assert_eq!(column.language, Some(MappingLanguage::Column { name: "lang".into() }));
        assert_eq!(column.term_type, "Literal");
    }

    #[wasm_bindgen_test]
    async fn triples_creates_the_view_over_the_tables_the_page_resolves() {
        let engine = engine();
        let options = options(&engine, "return '\"job\".\"' + t + '\"';");
        JsFuture::from(mapping().triples(options.into()).unwrap())
            .await
            .expect("created");
        let statements = seen(&engine);
        assert_eq!(statements.len(), 1, "{statements:?}");
        let sql = &statements[0];
        assert!(
            sql.starts_with("CREATE OR REPLACE VIEW \"job\".triples AS SELECT "),
            "{sql}"
        );
        assert!(sql.contains("FROM \"job\".\"Person\" AS \"t\""), "the table: {sql}");
        assert!(sql.contains("FROM (SELECT subject, n.value AS value, n.lang AS lang FROM (SELECT subject, unnest(nickname) AS n FROM \"job\".\"Person\"))"), "the query's table: {sql}");
    }

    #[wasm_bindgen_test]
    fn what_the_page_gives_is_checked_before_a_statement_is_sent() {
        let engine = engine();
        // `tables` throws.
        let error = mapping()
            .triples(options(&engine, "throw new Error('no such table ' + t)").into())
            .err()
            .expect("refused");
        assert!(format!("{error:?}").contains("no such table"), "{error:?}");
        // `tables` answers something that is not a SQL name.
        let error = mapping()
            .triples(options(&engine, "return 'Person) --'").into())
            .err()
            .expect("refused");
        assert!(format!("{error:?}").contains("Person"), "{error:?}");
        // The view's name is not a SQL name, `tables` is not a function, the signal is missing.
        let bad = options(&engine, "return t");
        Reflect::set(&bad, &"into".into(), &"a; DROP TABLE x".into()).unwrap();
        assert!(mapping().triples(bad.into()).is_err());
        let bad = options(&engine, "return t");
        Reflect::set(&bad, &"tables".into(), &"Person".into()).unwrap();
        assert!(mapping().triples(bad.into()).is_err());
        let bad = options(&engine, "return t");
        Reflect::delete_property(&bad, &"signal".into()).unwrap();
        assert!(mapping().triples(bad.into()).is_err());
        assert!(seen(&engine).is_empty(), "nothing was sent");
    }

    #[wasm_bindgen_test]
    fn a_mapping_that_is_not_read_is_an_error_that_names_it() {
        let graph_map = MAPPING.replace("rml:class ex:Person", "rml:graph ex:g");
        let error = Mapping::parse(graph_map, None).err().expect("refused");
        assert!(message(error).contains("rml:graph"), "names the property");
        let error = Mapping::parse(
            "@prefix rml: <http://w3id.org/rml/> . <#T> rml:subjectMap [".to_owned(),
            None,
        )
        .err()
        .expect("refused");
        let located = Reflect::get(&error, &"line".into()).unwrap();
        assert_eq!(located.as_f64(), Some(1.0), "a syntax error is placed");
    }

    #[wasm_bindgen_test]
    async fn a_base_is_the_documents_and_the_base_iri_of_generated_relative_iris() {
        let options = to_js(&ParseOptions {
            media_type: None,
            base: Some("http://tenant.example/".to_owned()),
        })
        .unwrap();
        let mapping = Mapping::parse(MAPPING.to_owned(), Some(options)).unwrap();
        let engine = engine();
        JsFuture::from(mapping.triples(options_for(&engine).into()).unwrap())
            .await
            .expect("created");
        let statements = seen(&engine);
        assert!(statements[0].contains("'http://tenant.example/'"), "{}", statements[0]);
        assert!(
            !statements[0].contains("'http://example.org/'"),
            "the default base is not used"
        );
    }

    fn options_for(engine: &JsValue) -> js_sys::Object {
        options(engine, "return '\"job\".\"' + t + '\"';")
    }
}
