//! What a mapping says about its tables, for a reader that wants the meaning
//! and not the rules: per logical table the classes, and per column the
//! predicate, the term type, the datatype, the language, and the annotations
//! the mapping graph carries about the class and the predicate IRIs
//! (RML-Core §3.2 allows "documentation in the form of rdfs:label,
//! rdfs:comment and similar properties").
//!
//! It reads the AST and nothing else, so it needs no SQL and runs wherever the
//! mapping was parsed.

use crate::ast::{
    Annotation, Constant, Expression, Iterable, LanguageMap, Mapping, Node, ObjectMap, Position, TermType,
};
use rudof_iri::IriS;

/// A mapping, per triples map (one per logical table or query).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub tables: Vec<Table>,
}

/// What one triples map says about the rows of its logical source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub triples_map: Node,
    pub source: Iterable,
    /// How a row is named: the subject map's expression.
    pub subject: Expression,
    /// The classes of a row's subject (§6.2), as IRIs.
    pub classes: Vec<IriS>,
    /// The annotations about those classes.
    pub annotations: Vec<Annotation>,
    /// The properties a row has: a predicate IRI the mapping states, with a
    /// value that is not another row's subject.
    pub columns: Vec<Column>,
    /// The properties that point at another triples map's subjects.
    pub relations: Vec<Relation>,
}

/// One predicate-object pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub predicate: IriS,
    /// Where the value comes from: a column (`Reference`), a template, or a
    /// constant.
    pub value: Expression,
    /// The term type the object map generates, with §8.4.1's default applied.
    pub term_type: TermType,
    /// The datatype the mapping states; `None` is the natural datatype of the
    /// column's SQL type (§11.1), which the mapping does not know.
    pub datatype: Option<IriS>,
    /// The language: a constant tag, or the column that holds it.
    pub language: Option<LanguageMap>,
    /// The annotations about the predicate.
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub predicate: IriS,
    pub parent: Node,
    pub annotations: Vec<Annotation>,
}

impl Mapping {
    /// The mapping as [`Model`]. Predicates that are not constant IRIs are left
    /// out: they name nothing before a row is read.
    pub fn model(&self) -> Model {
        let about = |iri: &IriS| -> Vec<Annotation> {
            self.annotations.iter().filter(|a| &a.subject == iri).cloned().collect()
        };
        let tables = self
            .triples_maps
            .iter()
            .map(|m| {
                let mut columns = Vec::new();
                let mut relations = Vec::new();
                for pom in &m.predicate_object_maps {
                    for predicate in &pom.predicates {
                        let Expression::Constant(Constant::Iri(predicate)) = &predicate.expression else {
                            continue;
                        };
                        for object in &pom.objects {
                            match object {
                                ObjectMap::Term(map) => columns.push(Column {
                                    predicate: predicate.clone(),
                                    value: map.expression.clone(),
                                    term_type: map.term_type(Position::Object),
                                    datatype: map.datatype.clone(),
                                    language: map.language.clone(),
                                    annotations: about(predicate),
                                }),
                                ObjectMap::Reference(r) => relations.push(Relation {
                                    predicate: predicate.clone(),
                                    parent: r.parent.clone(),
                                    annotations: about(predicate),
                                }),
                            }
                        }
                    }
                }
                Table {
                    triples_map: m.id.clone(),
                    source: m.logical_source.iterable.clone(),
                    subject: m.subject_map.term.expression.clone(),
                    classes: m.subject_map.classes.clone(),
                    annotations: m.subject_map.classes.iter().flat_map(&about).collect(),
                    columns,
                    relations,
                }
            })
            .collect();
        Model { tables }
    }
}
