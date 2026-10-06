//! The default `sh:resultMessage` texts: a multilingual catalog expressed as RDF.
//!
//! See `README.md` in this directory for the normative basis and for what is this
//! engine's own choice. In short: SHACL §3.6.2.7 lets a processor "automatically
//! generate other values for `sh:resultMessage`" when a constraint has no
//! `sh:message`; this engine generates them from `sh:message` literals declared on
//! the constraint *component* (the SHACL-SPARQL mechanism), with `{$param}`
//! placeholders named after the component's own parameters.

use crate::types::MessageMap;
use rudof_iri::IriS;
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::term::Object;
use rudof_rdf::term::Triple;
use rudof_rdf::term::literal::Lang;
use rudof_rdf::vocab::ShaclVocab;
use rudof_rdf::{NeighsRDF, RDFFormat, Rdf};
use std::collections::HashMap;
use std::sync::OnceLock;

/// The built-in catalog: one file per language, all data.
const BUILTIN: [&str; 3] = [
    include_str!("messages.en.ttl"),
    include_str!("messages.es.ttl"),
    include_str!("messages.ca.ttl"),
];

/// A catalog document that could not be read.
#[derive(Debug, thiserror::Error)]
#[error("message catalog: {0}")]
pub struct MessageError(String);

/// One piece of a message template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    /// `{$name}` or `{?name}`: the two are interchangeable (SHACL-SPARQL §5.1).
    Var(String),
}

/// A parsed `sh:message` template.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Template(Vec<Piece>);

impl Template {
    fn parse(text: &str) -> Self {
        let mut pieces = Vec::new();
        let mut literal = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            literal.push_str(&rest[..open]);
            let body = &rest[open + 1..];
            match placeholder(body) {
                Some(name) => {
                    if !literal.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut literal)));
                    }
                    pieces.push(Piece::Var(name.to_string()));
                    // sigil + name + `}`
                    rest = &body[name.len() + 2..];
                },
                None => {
                    // A brace that opens no placeholder is ordinary text.
                    literal.push('{');
                    rest = body;
                },
            }
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            pieces.push(Piece::Text(literal));
        }
        Self(pieces)
    }

    /// Substitute each placeholder with what `resolve` yields; a placeholder it
    /// does not know is kept as written.
    fn render(&self, resolve: &impl Fn(&str) -> Option<String>) -> String {
        self.0
            .iter()
            .map(|piece| match piece {
                Piece::Text(t) => t.clone(),
                Piece::Var(v) => resolve(v).unwrap_or_else(|| format!("{{${v}}}")),
            })
            .collect()
    }
}

/// The variable name of a placeholder whose body follows an opening `{`:
/// `$name}` or `?name}`, with `name` a SPARQL-style variable name.
fn placeholder(body: &str) -> Option<&str> {
    let name = body.strip_prefix(['$', '?'])?.split_once('}')?.0;
    let mut chars = name.chars();
    let starts = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    (starts && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(name)
}

/// Message templates per constraint component and language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageCatalog {
    entries: HashMap<IriS, HashMap<Option<Lang>, Template>>,
}

impl MessageCatalog {
    /// The catalog every engine starts from, parsed once per process.
    pub fn builtin() -> &'static MessageCatalog {
        static BUILTIN_CATALOG: OnceLock<MessageCatalog> = OnceLock::new();
        BUILTIN_CATALOG.get_or_init(|| {
            let mut catalog = MessageCatalog::default();
            for document in BUILTIN {
                catalog
                    .load(document, &RDFFormat::Turtle)
                    .expect("the built-in message catalog is valid Turtle");
            }
            catalog
        })
    }

    /// Add the `sh:message` literals of an RDF document. A message replaces the
    /// one already held for the same component and language; the rest is kept, so
    /// a document can add a language or reword one message without repeating the
    /// others. Nothing is added when the document does not parse.
    pub fn load(&mut self, text: &str, format: &RDFFormat) -> Result<(), MessageError> {
        let graph = OxigraphInMemory::from_str(text, format, None, &ReaderMode::Strict)
            .map_err(|e| MessageError(e.to_string()))?;
        let triples = graph
            .triples_with_predicate(&ShaclVocab::sh_message().into())
            .map_err(|e| MessageError(e.to_string()))?;
        for triple in triples {
            let (subject, _, object) = triple.into_components();
            let Ok(Object::Iri(component)) =
                OxigraphInMemory::term_as_object(&OxigraphInMemory::subject_as_term(&subject))
            else {
                continue;
            };
            let Ok(Object::Literal(message)) = OxigraphInMemory::term_as_object(&object) else {
                continue;
            };
            self.entries
                .entry(component)
                .or_default()
                .insert(message.lang(), Template::parse(&message.lexical_form()));
        }
        Ok(())
    }

    /// The messages for a result of `component`: one per language the catalog
    /// holds for it (never two with the same tag), each with its placeholders
    /// replaced by what `resolve` returns. A component the catalog does not name
    /// takes the messages of `sh:ConstraintComponent`, the class of all of them;
    /// with none there either, the result carries no message.
    pub fn render(&self, component: &IriS, resolve: impl Fn(&str) -> Option<String>) -> MessageMap {
        self.entries
            .get(component)
            .or_else(|| self.entries.get(&ShaclVocab::sh_constraint_component()))
            .into_iter()
            .flatten()
            .fold(MessageMap::new(), |map, (lang, template)| {
                map.with_message(lang.clone(), template.render(&resolve))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(tag: &str) -> Option<Lang> {
        Some(Lang::new(tag).unwrap())
    }

    fn min_count() -> IriS {
        ShaclVocab::sh_min_count_constraint_component()
    }

    #[test]
    fn placeholders_take_both_sigils_and_keep_stray_braces() {
        let t = Template::parse("a {$x} b {?y} {not} {$} {$z");
        let out = t.render(&|n| (n != "z").then(|| n.to_uppercase()));
        assert_eq!(out, "a X b Y {not} {$} {$z");
    }

    #[test]
    fn unknown_variable_is_kept_as_written() {
        assert_eq!(Template::parse("{$nope}").render(&|_| None), "{$nope}");
    }

    #[test]
    fn builtin_covers_every_language_and_never_repeats_one() {
        let map = MessageCatalog::builtin().render(&min_count(), |_| Some("2".into()));
        assert_eq!(map.messages().len(), 3);
        for tag in ["en", "es", "ca"] {
            assert!(map.get(lang(tag).as_ref()).unwrap().contains('2'), "{tag}");
        }
    }

    #[test]
    fn later_document_wins_per_component_and_language() {
        let mut c = MessageCatalog::builtin().clone();
        let doc = r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
            sh:MinCountConstraintComponent sh:message "Need {$minCount}"@en , "Faut {$minCount}"@fr ."#;
        c.load(doc, &RDFFormat::Turtle).unwrap();
        let map = c.render(&min_count(), |_| Some("2".into()));
        assert_eq!(map.get(lang("en").as_ref()).unwrap(), "Need 2");
        assert_eq!(map.get(lang("fr").as_ref()).unwrap(), "Faut 2");
        assert!(map.get(lang("es").as_ref()).unwrap().contains('2'));
        assert_eq!(map.messages().len(), 4);
    }

    #[test]
    fn a_broken_document_adds_nothing() {
        let mut c = MessageCatalog::default();
        assert!(c.load("this is not turtle", &RDFFormat::Turtle).is_err());
        assert_eq!(c, MessageCatalog::default());
    }

    #[test]
    fn unnamed_component_falls_back_to_the_generic_entry() {
        let other = IriS::new_unchecked("http://example.org/OtherConstraintComponent");
        let map = MessageCatalog::builtin().render(&other, |_| None);
        assert_eq!(map.get(lang("en").as_ref()).unwrap(), "Invalid value");
        assert!(MessageCatalog::default().render(&other, |_| None).messages().is_empty());
    }
}

/// The values a message template can name: a component's parameters, by the local
/// name of the SHACL parameter (`minCount`, `datatype`, ...), already rendered as
/// text. `{$value}` is not among them; it belongs to each result.
pub type Parameters = Vec<(&'static str, String)>;

/// A term as the text of a message: an IRI in its prefixed form when the shapes
/// graph declares a prefix for it, a literal by its lexical form.
pub fn display(schema: &crate::ir::IRSchema, term: &Object) -> String {
    match term {
        Object::Iri(iri) => schema.prefix_map().qualify(iri),
        Object::Literal(lit) => lit.lexical_form(),
        other => other.to_string(),
    }
}

/// The message parameters of `component`, named after its SHACL parameters.
/// The logical, shape-based and status components name none; the two of
/// `sh:qualifiedValueShape` are named by the check that reports them.
pub fn parameters(component: &crate::ir::IRComponent, schema: &crate::ir::IRSchema) -> Parameters {
    use crate::ir::IRComponent as C;
    let iri = |i: &IriS| display(schema, &Object::Iri(i.clone()));
    let list = |items: Vec<String>| items.join(", ");
    match component {
        C::Class(c) => vec![("class", display(schema, c))],
        C::Datatype(ds) => vec![(
            "datatype",
            list(ds.iter().map(|d| schema.prefix_map().qualify(d)).collect()),
        )],
        C::NodeKind(k) => vec![("nodeKind", k.to_string())],
        C::MinCount(n) => vec![("minCount", n.to_string())],
        C::MaxCount(n) => vec![("maxCount", n.to_string())],
        C::MinExclusive(l) => vec![("minExclusive", l.lexical_form())],
        C::MaxExclusive(l) => vec![("maxExclusive", l.lexical_form())],
        C::MinInclusive(l) => vec![("minInclusive", l.lexical_form())],
        C::MaxInclusive(l) => vec![("maxInclusive", l.lexical_form())],
        C::MinLength(n) => vec![("minLength", n.to_string())],
        C::MaxLength(n) => vec![("maxLength", n.to_string())],
        C::Pattern(p) => vec![("pattern", p.pattern().to_string())],
        C::LanguageIn(ls) => vec![("languageIn", list(ls.iter().map(ToString::to_string).collect()))],
        C::Equals(p) => vec![("equals", iri(p))],
        C::Disjoint(p) => vec![("disjoint", iri(p))],
        C::LessThan(p) => vec![("lessThan", iri(p))],
        C::LessThanOrEquals(p) => vec![("lessThanOrEquals", iri(p))],
        C::HasValue(v) => vec![("hasValue", display(schema, v))],
        C::In(vs) => vec![("in", list(vs.iter().map(|v| display(schema, v)).collect()))],
        C::UniqueLang(_)
        | C::Or(_)
        | C::And(_)
        | C::Not(_)
        | C::Xone(_)
        | C::If(_)
        | C::Node(_)
        | C::QualifiedValueShape(_)
        | C::Closed(_)
        | C::Deactivated(_)
        | C::BasicSparql(_) => Vec::new(),
    }
}

/// The `sh:resultMessage` set of a result of `component`.
///
/// `sh:message` is declared on the *shape*, so every result of the shape owes
/// the author their text, language tags included: "If a shape has at least one
/// value for `sh:message` in the shapes graph, then all validation results
/// produced as a result of the shape will have **exactly these messages** as
/// their value of `sh:resultMessage`" (SHACL §2.1.5). So the author's set is
/// copied and nothing is added to it.
///
/// Only "in cases where a constraint does not have any values for `sh:message`"
/// may the processor "automatically generate other values" (§3.6.2.7): those come
/// from the schema's [`MessageCatalog`], one per language it holds for the
/// component, with `{$name}` replaced from `parameters` and `{$value}` from the
/// value node.
pub fn result_message(
    schema: &crate::ir::IRSchema,
    shape: &crate::ir::IRShape,
    component: &IriS,
    parameters: &[(&str, String)],
    value: Option<&Object>,
) -> MessageMap {
    match shape.message() {
        Some(author) if !author.messages().is_empty() => author.clone(),
        _ => schema.messages().render(component, |name| match name {
            "value" => value.map(|v| display(schema, v)),
            _ => parameters.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()),
        }),
    }
}
