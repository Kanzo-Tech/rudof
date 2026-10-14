//! Writing a mapping as its graph.
//!
//! Constant-valued term maps are written with the shortcut properties of
//! RML-Core §8.1.1 (`rml:predicate`, `rml:object`, `rml:datatype`,
//! `rml:language`) and the others as term maps; the parser reads both. Every
//! logical source names its `rml:source` (RML-IO §3.1), a blank `rml:Source`
//! when the mapping names none. Annotations follow as plain triples about the
//! IRIs they annotate (§3.2).

use crate::Error;
use crate::ast::{
    Constant, Expression, Iterable, LanguageMap, Literal, LiteralKind, Mapping, Node, ObjectMap, TermMap, TermType,
};
use crate::vocab::{self, rml, term as t, xsd};
use prefixmap::IriRef;
use rudof_iri::IriS;
use rudof_rdf::backend::OxigraphInMemory;
use rudof_rdf::term::literal::{ConcreteLiteral, Lang};
use rudof_rdf::{BuildRDF, RDFFormat};
use std::collections::HashMap;

/// The mapping as Turtle (RML-Core §3.3), prefixed `rml:`, `rdf:` and `xsd:`.
pub fn write_turtle(mapping: &Mapping) -> Result<String, Error> {
    let graph: OxigraphInMemory = write(mapping)?;
    let mut out = Vec::new();
    graph
        .serialize(&RDFFormat::Turtle, &mut out)
        .map_err(|e| Error::Graph(e.to_string()))?;
    String::from_utf8(out).map_err(|e| Error::Graph(e.to_string()))
}

/// The mapping built into a new graph. A mapping that is not valid
/// ([`Mapping::validate`]) is refused.
pub fn write<R: BuildRDF>(mapping: &Mapping) -> Result<R, Error> {
    mapping.validate()?;
    let mut w = Writer::<R> {
        rdf: R::empty(),
        blanks: HashMap::new(),
    };
    for (alias, namespace) in [("rml", vocab::RML), ("rdf", vocab::RDF), ("xsd", vocab::XSD)] {
        w.rdf
            .add_prefix(alias, &IriS::new_unchecked(namespace))
            .map_err(graph_error::<R>)?;
    }
    for m in &mapping.triples_maps {
        w.triples_map(m)?;
    }
    for a in &mapping.annotations {
        let object = w.constant(&a.object)?;
        w.rdf
            .add_triple(a.subject.clone(), a.predicate.clone(), object)
            .map_err(graph_error::<R>)?;
    }
    Ok(w.rdf)
}

fn graph_error<R: BuildRDF>(e: R::Err) -> Error {
    Error::Graph(e.to_string())
}

struct Writer<R: BuildRDF> {
    rdf: R,
    blanks: HashMap<String, R::Subject>,
}

impl<R: BuildRDF> Writer<R> {
    fn subject(&mut self, node: &Node) -> Result<R::Subject, Error> {
        match node {
            Node::Iri(iri) => Ok(R::Subject::from(iri.clone())),
            Node::Blank(label) => {
                if let Some(s) = self.blanks.get(label) {
                    return Ok(s.clone());
                }
                let s = self.fresh()?;
                self.blanks.insert(label.clone(), s.clone());
                Ok(s)
            },
        }
    }

    fn fresh(&mut self) -> Result<R::Subject, Error> {
        Ok(R::Subject::from(self.rdf.add_bnode().map_err(graph_error::<R>)?))
    }

    fn put(&mut self, s: &R::Subject, p: IriS, o: R::Term) -> Result<(), Error> {
        self.rdf.add_triple(s.clone(), p, o).map_err(graph_error::<R>)
    }

    /// `s rml:{property} <iri>`.
    fn iri(&mut self, s: &R::Subject, property: &str, iri: &IriS) -> Result<(), Error> {
        self.put(s, rml(property), R::Term::from(iri.clone()))
    }

    /// `s rml:{property} "text"`.
    fn text(&mut self, s: &R::Subject, property: &str, text: &str) -> Result<(), Error> {
        self.put(
            s,
            rml(property),
            R::Term::from(R::Literal::from(ConcreteLiteral::str(text))),
        )
    }

    /// A new blank node `o`, and `s rml:{property} o`.
    fn child(&mut self, s: &R::Subject, property: &str) -> Result<R::Subject, Error> {
        let o = self.fresh()?;
        self.put(s, rml(property), R::Term::from(o.clone()))?;
        Ok(o)
    }

    fn typed(&mut self, s: &R::Subject, class: &str) -> Result<(), Error> {
        self.rdf.add_type(s.clone(), rml(class)).map_err(graph_error::<R>)
    }

    fn constant(&mut self, c: &Constant) -> Result<R::Term, Error> {
        Ok(match c {
            Constant::Iri(iri) => R::Term::from(iri.clone()),
            Constant::Literal(l) => R::Term::from(R::Literal::from(concrete(l)?)),
        })
    }

    fn triples_map(&mut self, m: &crate::ast::TriplesMap) -> Result<(), Error> {
        let s = self.subject(&m.id)?;
        self.typed(&s, t::TRIPLES_MAP)?;

        let source = self.child(&s, t::P_LOGICAL_SOURCE)?;
        self.typed(&source, t::LOGICAL_SOURCE)?;
        match &m.logical_source.source {
            Some(iri) => self.iri(&source, t::P_SOURCE, iri)?,
            None => {
                let anonymous = self.child(&source, t::P_SOURCE)?;
                self.typed(&anonymous, t::SOURCE)?;
            },
        }
        let (formulation, iterator) = match &m.logical_source.iterable {
            Iterable::Sql2008Table(name) => (t::SQL2008_TABLE, name),
            Iterable::Sql2008Query(query) => (t::SQL2008_QUERY, query),
        };
        self.iri(&source, t::P_REFERENCE_FORMULATION, &rml(formulation))?;
        self.text(&source, t::P_ITERATOR, iterator)?;

        let subject_map = self.child(&s, t::P_SUBJECT_MAP)?;
        self.term_map(&subject_map, &m.subject_map.term)?;
        for class in &m.subject_map.classes {
            self.iri(&subject_map, t::P_CLASS, class)?;
        }

        for pom in &m.predicate_object_maps {
            let p = self.child(&s, t::P_PREDICATE_OBJECT_MAP)?;
            for predicate in &pom.predicates {
                match &predicate.expression {
                    Expression::Constant(Constant::Iri(iri)) => self.iri(&p, t::P_PREDICATE, iri)?,
                    _ => {
                        let map = self.child(&p, t::P_PREDICATE_MAP)?;
                        self.term_map(&map, predicate)?;
                    },
                }
            }
            for object in &pom.objects {
                match object {
                    ObjectMap::Term(TermMap {
                        expression: Expression::Constant(c),
                        ..
                    }) => {
                        let o = self.constant(c)?;
                        self.put(&p, rml(t::P_OBJECT), o)?;
                    },
                    ObjectMap::Term(map) => {
                        let o = self.child(&p, t::P_OBJECT_MAP)?;
                        self.term_map(&o, map)?;
                    },
                    ObjectMap::Reference(r) => {
                        let o = self.child(&p, t::P_OBJECT_MAP)?;
                        let parent = self.subject(&r.parent)?;
                        self.put(&o, rml(t::P_PARENT_TRIPLES_MAP), R::Term::from(parent))?;
                        for jc in &r.join_conditions {
                            let j = self.child(&o, t::P_JOIN_CONDITION)?;
                            self.text(&j, t::P_CHILD, &jc.child)?;
                            self.text(&j, t::P_PARENT, &jc.parent)?;
                        }
                    },
                }
            }
        }
        Ok(())
    }

    /// The properties of a term map on `node`.
    fn term_map(&mut self, node: &R::Subject, map: &TermMap) -> Result<(), Error> {
        match &map.expression {
            Expression::Constant(c) => {
                let o = self.constant(c)?;
                self.put(node, rml(t::P_CONSTANT), o)?;
            },
            Expression::Reference(column) => self.text(node, t::P_REFERENCE, column)?,
            Expression::Template(template) => self.text(node, t::P_TEMPLATE, &template.to_string())?,
        }
        if let Some(term_type) = map.term_type {
            let local = match term_type {
                TermType::Iri => t::IRI,
                TermType::UnsafeIri => t::UNSAFE_IRI,
                TermType::BlankNode => t::BLANK_NODE,
                TermType::Literal => t::LITERAL,
            };
            self.iri(node, t::P_TERM_TYPE, &rml(local))?;
        }
        if let Some(datatype) = &map.datatype {
            self.iri(node, t::P_DATATYPE, datatype)?;
        }
        match &map.language {
            None => {},
            Some(LanguageMap::Constant(tag)) => self.text(node, t::P_LANGUAGE, tag)?,
            Some(LanguageMap::Reference(column)) => {
                let language = self.child(node, t::P_LANGUAGE_MAP)?;
                self.text(&language, t::P_REFERENCE, column)?;
            },
        }
        Ok(())
    }
}

fn concrete(l: &Literal) -> Result<ConcreteLiteral, Error> {
    Ok(match &l.kind {
        LiteralKind::Language(tag) => ConcreteLiteral::lang_str(
            &l.lexical_form,
            Lang::new(tag.clone()).map_err(|e| Error::invalid(&l.lexical_form, &e.to_string()))?,
        ),
        LiteralKind::Datatype(d) if *d == xsd("string") => ConcreteLiteral::str(&l.lexical_form),
        LiteralKind::Datatype(d) => ConcreteLiteral::lit_datatype(&l.lexical_form, &IriRef::iri(d.clone())),
    })
}
