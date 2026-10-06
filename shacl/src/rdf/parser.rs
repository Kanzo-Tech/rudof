use crate::ast::{ASTSchema, ASTShape};
use crate::rdf::State;
use crate::rdf::error::ShaclParserError;
use crate::rdf::parsers::{node_shape, property_shape};
use crate::types::{Annotation, Annotations, MessageMap, Severity};
use itertools::Itertools;
use rudof_iri::IriS;
use rudof_rdf::parser::RDFParse;
use rudof_rdf::parser::rdf_node_parser::constructors::{ListParser, SuccessParser};
use rudof_rdf::parser::rdf_node_parser::{ParserExt, RDFNodeParse};
use rudof_rdf::term::Triple;
use rudof_rdf::term::{IriOrBlankNode, Object};
use rudof_rdf::vocab::{RdfVocab, RdfsVocab, ShaclVocab};
use rudof_rdf::{Any, Matcher, NeighsRDF};
use std::collections::{HashMap, HashSet};

pub struct ShaclParser<RDF: NeighsRDF> {
    rdf_parser: RDFParse<RDF>,
    shapes: HashMap<Object, ASTShape>,
}

impl<RDF: NeighsRDF + 'static> ShaclParser<RDF> {
    pub fn new(rdf: RDF) -> Self {
        Self {
            rdf_parser: RDFParse::new(rdf),
            shapes: HashMap::new(),
        }
    }

    pub fn parse(&mut self) -> Result<ASTSchema, ShaclParserError> {
        let pm = self.rdf_parser.prefixmap().unwrap_or_default();

        let mut state: State = self.shapes_candidates()?.into();
        let mut annotations = self.annotations()?;
        let mut by_types = None;

        while let Some(node) = state.pop_pending() {
            if !self.shapes.contains_key(&node) {
                self.rdf_parser.set_focus(&node.clone().into());
                let mut shape = shape().parse_focused(&mut self.rdf_parser.ctx())?;
                if let Some(a) = annotations.remove(&node) {
                    shape = shape.with_annotations(a);
                }
                if shape.closed_by_types() {
                    let map = match &by_types {
                        Some(map) => map,
                        None => by_types.insert(self.properties_by_type()?),
                    };
                    shape = shape.with_properties_by_type(map);
                }
                self.shapes.insert(node, shape);
            }
        }

        Ok(
            ASTSchema::new().with_prefixmap(pm).with_shapes(self.shapes.clone()), // TODO - Maybe avoid the shapes clone
        )
    }

    /// The pairs of `predicate`, as objects.
    fn pairs(&self, predicate: IriS) -> Result<Vec<(Object, Object)>, ShaclParserError> {
        let rdf = self.rdf_parser.rdf();
        rdf.triples_with_predicate(&predicate.into())
            .map_err(|e| ShaclParserError::TriplesLookupError(e.to_string()))?
            .map(|t| {
                let (s, _, o) = t.into_components();
                Ok((RDF::subject_as_node(&s)?, RDF::term_as_object(&o)?))
            })
            .collect()
    }

    /// What the reifiers of each shape's triples say about its constraints
    /// (SHACL 1.2 §2.1.3–§2.1.5): for every `r rdf:reifies <<( s p o )>>`, the
    /// `sh:severity`, `sh:message` and `sh:deactivated` of `r`, under `s` and `p`.
    fn annotations(&self) -> Result<HashMap<Object, Annotations>, ShaclParserError> {
        let values = |predicate: IriS| -> Result<HashMap<Object, Vec<Object>>, ShaclParserError> {
            Ok(self.pairs(predicate)?.into_iter().into_group_map())
        };
        let severities = values(ShaclVocab::sh_severity())?;
        let messages = values(ShaclVocab::sh_message())?;
        let deactivated = values(ShaclVocab::sh_deactivated())?;
        let mut out: HashMap<Object, Annotations> = HashMap::new();
        for (reifier, triple) in self.pairs(RdfVocab::rdf_reifies())? {
            let Object::Triple {
                subject,
                predicate,
                object,
            } = triple
            else {
                continue;
            };
            let annotation = Annotation {
                severity: severities.get(&reifier).and_then(|vs| match vs.first() {
                    Some(Object::Iri(iri)) => Some(Severity::from(iri)),
                    _ => None,
                }),
                message: messages.get(&reifier).map(|vs| {
                    vs.iter().fold(MessageMap::new(), |map, v| match v {
                        Object::Literal(lit) => map.with_message(lit.lang(), lit.lexical_form()),
                        _ => map,
                    })
                }),
                deactivated: deactivated
                    .get(&reifier)
                    .is_some_and(|vs| vs.contains(&Object::boolean(true))),
            };
            let shape = match *subject {
                IriOrBlankNode::Iri(iri) => Object::Iri(iri),
                IriOrBlankNode::BlankNode(b) => Object::BlankNode(b),
            };
            out.entry(shape).or_default().push((predicate, *object, annotation));
        }
        Ok(out)
    }

    /// `collectProperties` (SHACL 1.2 §8.4.1) of every node of the shapes graph
    /// that can permit a property: the properties a value node of that type
    /// may have under `sh:closed sh:ByTypes`, besides `rdf:type`.
    fn properties_by_type(&self) -> Result<Vec<(IriS, Vec<IriS>)>, ShaclParserError> {
        let edges = |predicate: IriS| -> Result<HashMap<Object, Vec<Object>>, ShaclParserError> {
            Ok(self.pairs(predicate)?.into_iter().into_group_map())
        };
        let sub_class_of = edges(RdfsVocab::rdfs_subclass_of_str())?;
        let node = edges(ShaclVocab::sh_node())?;
        let property = edges(ShaclVocab::sh_property())?;
        let path = edges(ShaclVocab::sh_path())?;
        let types = edges(RdfVocab::rdf_type())?;
        let mut targeting: HashMap<Object, Vec<Object>> = HashMap::new();
        for (shape, class) in self.pairs(ShaclVocab::sh_target_class())? {
            targeting.entry(class).or_default().push(shape);
        }
        // SHACL instances of a class within the shapes graph.
        let instance_of = |node: &Object, roots: &[IriS]| {
            let mut seen: HashSet<&Object> = HashSet::new();
            let mut pending: Vec<&Object> = types.get(node).into_iter().flatten().collect();
            while let Some(class) = pending.pop() {
                if matches!(class, Object::Iri(iri) if roots.contains(iri)) {
                    return true;
                }
                if seen.insert(class) {
                    pending.extend(sub_class_of.get(class).into_iter().flatten());
                }
            }
            false
        };
        let class_roots = [RdfsVocab::rdfs_class(), ShaclVocab::sh_shape_class()];
        let shape_roots = [ShaclVocab::sh_node_shape(), ShaclVocab::sh_shape_class()];
        let collect = |start: &Object| {
            let mut properties: Vec<IriS> = Vec::new();
            let mut seen: HashSet<Object> = HashSet::new();
            let mut pending = vec![start.clone()];
            while let Some(s) = pending.pop() {
                if !seen.insert(s.clone()) {
                    continue;
                }
                for p in property.get(&s).into_iter().flatten() {
                    for path in path.get(p).into_iter().flatten() {
                        if let Object::Iri(iri) = path
                            && !properties.contains(iri)
                        {
                            properties.push(iri.clone());
                        }
                    }
                }
                if instance_of(&s, &class_roots) {
                    pending.extend(sub_class_of.get(&s).into_iter().flatten().cloned());
                    pending.extend(targeting.get(&s).into_iter().flatten().cloned());
                }
                if instance_of(&s, &shape_roots) {
                    pending.extend(node.get(&s).into_iter().flatten().cloned());
                }
            }
            properties.sort();
            properties
        };
        let mut out: Vec<(IriS, Vec<IriS>)> = types
            .keys()
            .chain(targeting.keys())
            .chain(property.keys())
            .filter_map(|t| match t {
                Object::Iri(iri) => Some(iri.clone()),
                _ => None,
            })
            .unique()
            .map(|t| {
                let properties = collect(&Object::Iri(t.clone()));
                (t, properties)
            })
            .filter(|(_, properties)| !properties.is_empty())
            .collect();
        out.sort();
        Ok(out)
    }

    /// Shapes candidates are defined in Appendix A of SHACL spec (Syntax rules)
    /// The text is:
    /// A shape is an IRI or blank node s that fulfills at least one of the following conditions in the shapes graph:
    /// - s is a SHACL instance of sh:NodeShape or sh:PropertyShape.
    /// - s is subject of a triple that has sh:targetClass, sh:targetNode, sh:targetObjectsOf, sh:targetSubjectsOf or sh:targetWhere as predicate.
    /// - s is subject of a triple that has a parameter as predicate.
    /// - s is a value of sh:targetWhere (SHACL 1.2).
    /// - s is a value of a shape-expecting, non-list-taking parameter such as sh:node,
    ///   or a member of a SHACL list that is a value of a shape-expecting and list-taking parameter such as sh:or.
    fn shapes_candidates(&mut self) -> Result<Vec<Object>, ShaclParserError> {
        // instances of `sh:NodeShape`
        let mut node_shapes_instances = self.get_triples::<_, RDF::IRI, RDF::Term>(
            &Any,
            &RdfVocab::rdf_type().into(),
            &ShaclVocab::sh_node_shape().into(),
        )?;
        // instances of `sh:PropertyShape`
        let property_shapes_instances = self.get_triples::<_, RDF::IRI, RDF::Term>(
            &Any,
            &RdfVocab::rdf_type().into(),
            &ShaclVocab::sh_property_shape().into(),
        )?;
        // instances of `sh:Shape`
        let shape_instances = self.get_triples::<_, RDF::IRI, RDF::Term>(
            &Any,
            &RdfVocab::rdf_type().into(),
            &ShaclVocab::sh_shape().into(),
        )?;
        // instances of `sh:ShapeClass`, a subclass of `sh:NodeShape` (SHACL 1.2)
        let shape_class_instances = self.get_triples::<_, RDF::IRI, RDF::Term>(
            &Any,
            &RdfVocab::rdf_type().into(),
            &ShaclVocab::sh_shape_class().into(),
        )?;
        // subjects of sh:targetClass
        let subjects_target_class =
            self.get_triples::<_, RDF::IRI, _>(&Any, &ShaclVocab::sh_target_class().into(), &Any)?;
        // subjects of sh:targetSubjectsOf
        let subjects_target_subjects_of =
            self.get_triples::<_, RDF::IRI, _>(&Any, &ShaclVocab::sh_target_subjects_of().into(), &Any)?;
        // subjects of sh:targetObjectsOf
        let subjects_target_objects_of =
            self.get_triples::<_, RDF::IRI, _>(&Any, &ShaclVocab::sh_target_objects_of().into(), &Any)?;
        // subjects of sh:targetWhere
        let subjects_target_where =
            self.get_triples::<_, RDF::IRI, _>(&Any, &ShaclVocab::sh_target_where().into(), &Any)?;
        // subjects of sh:targetNode
        let subjects_target_node =
            self.get_triples::<_, RDF::IRI, _>(&Any, &ShaclVocab::sh_target_node().into(), &Any)?;
        // Search shape expecting parameters: https://www.w3.org/TR/shacl12-core/#dfn-shape-expecting
        // values of `sh:targetWhere`, which are shapes
        let target_where_values = self.objects_with_predicate(&ShaclVocab::sh_target_where().into())?;
        // elements of `sh:and` list
        let sh_and_values = self.get_triples_list(&ShaclVocab::sh_and().into(), "sh:and", |v, ctx| {
            ShaclParserError::ValueNotExpected {
                iri: ctx.to_string(),
                expected: "subject".to_string(),
                found: v.to_string(),
            }
        })?;
        // elements of `sh:or` list
        let sh_or_values = self.get_triples_list(&ShaclVocab::sh_or().into(), "sh:or", |v, ctx| {
            ShaclParserError::ValueNotExpected {
                iri: ctx.to_string(),
                expected: "object".to_string(),
                found: v.to_string(),
            }
        })?;
        // elements of `sh:not` list
        let sh_not_values = self.objects_with_predicate(&ShaclVocab::sh_not().into())?;
        // subjects with property `sh:property`
        let subjects_property = self.objects_with_predicate(&ShaclVocab::sh_property().into())?;
        // elements of `sh:node` list
        let sh_qualified_value_shape_nodes =
            self.objects_with_predicate(&ShaclVocab::sh_qualified_value_shape().into())?;
        // elements of `sh:node` list
        let sh_node_values = self.objects_with_predicate(&ShaclVocab::sh_node().into())?;
        // values of the shape-expecting parameters of SHACL 1.2
        let sh_member_shape_values = self.objects_with_predicate(&ShaclVocab::sh_member_shape().into())?;
        let sh_some_value_values = self.objects_with_predicate(&ShaclVocab::sh_some_value().into())?;
        let sh_node_by_expression_values = self.objects_with_predicate(&ShaclVocab::sh_node_by_expression().into())?;
        // elements of `sh:xone` list
        let sh_xone_values = self.get_triples_list(&ShaclVocab::sh_xone().into(), "sh:xone", |v, ctx| {
            ShaclParserError::ValueNotExpected {
                iri: ctx.to_string(),
                expected: "subject".to_string(),
                found: v.to_string(),
            }
        })?;
        // elements of `sh:reifierShape` list
        let sh_reifier_shape_values = self.objects_with_predicate(&ShaclVocab::sh_reifier_shape().into())?;
        // objects of `sh:if` / `sh:then` / `sh:else` (SHACL-AF conditional)
        let sh_if_values = self.objects_with_predicate(&ShaclVocab::sh_if().into())?;
        let sh_then_values = self.objects_with_predicate(&ShaclVocab::sh_then().into())?;
        let sh_else_values = self.objects_with_predicate(&ShaclVocab::sh_else().into())?;

        node_shapes_instances.extend(property_shapes_instances);
        node_shapes_instances.extend(shape_instances);
        node_shapes_instances.extend(shape_class_instances);
        node_shapes_instances.extend(sh_member_shape_values);
        node_shapes_instances.extend(sh_some_value_values);
        node_shapes_instances.extend(sh_node_by_expression_values);
        node_shapes_instances.extend(subjects_target_class);
        node_shapes_instances.extend(subjects_target_subjects_of);
        node_shapes_instances.extend(subjects_target_objects_of);
        node_shapes_instances.extend(subjects_target_node);
        node_shapes_instances.extend(subjects_target_where);
        node_shapes_instances.extend(target_where_values);
        node_shapes_instances.extend(sh_and_values);
        node_shapes_instances.extend(sh_or_values);
        node_shapes_instances.extend(sh_not_values);
        node_shapes_instances.extend(subjects_property);
        node_shapes_instances.extend(sh_qualified_value_shape_nodes);
        node_shapes_instances.extend(sh_node_values);
        node_shapes_instances.extend(sh_xone_values);
        node_shapes_instances.extend(sh_reifier_shape_values);
        node_shapes_instances.extend(sh_if_values);
        node_shapes_instances.extend(sh_then_values);
        node_shapes_instances.extend(sh_else_values);

        Ok(node_shapes_instances
            .into_iter()
            .map(|s| RDF::subject_as_node(&s))
            .try_collect()?)
    }

    fn get_triples<S, P, O>(&self, s: &S, p: &P, o: &O) -> Result<HashSet<RDF::Subject>, ShaclParserError>
    where
        S: Matcher<RDF::Subject>,
        P: Matcher<RDF::IRI>,
        O: Matcher<RDF::Term>,
    {
        Ok(self
            .rdf_parser
            .rdf()
            .triples_matching(s, p, o)
            .map_err(|e| ShaclParserError::TriplesLookupError(e.to_string()))?
            .map(Triple::into_subject)
            .collect())
    }

    fn get_triples_list(
        &mut self,
        pred: &RDF::IRI,
        context: &str,
        err_fn: impl Fn(RDF::Term, &str) -> ShaclParserError,
    ) -> Result<HashSet<RDF::Subject>, ShaclParserError> {
        let mut rs = HashSet::new();

        for subject in self.objects_with_predicate(pred)? {
            self.rdf_parser.set_focus(&subject.into());
            let vs = ListParser::new().parse_focused(&mut self.rdf_parser.ctx())?;
            for v in vs {
                if let Ok(subj) = term_to_subject::<RDF>(&v, context) {
                    rs.insert(subj);
                } else {
                    return Err(err_fn(v, context));
                }
            }
        }

        Ok(rs)
    }

    fn objects_with_predicate(&self, pred: &RDF::IRI) -> Result<HashSet<RDF::Subject>, ShaclParserError> {
        let msg = format!("objects with predicate {pred}");
        let subjects = self
            .rdf_parser
            .rdf()
            .triples_with_predicate(pred)
            .map_err(|e| ShaclParserError::TriplesLookupError(e.to_string()))?
            .map(Triple::into_object)
            .flat_map(|t| term_to_subject::<RDF>(&t, msg.as_str()))
            .collect();
        Ok(subjects)
    }
}

fn term_to_subject<RDF: NeighsRDF>(term: &RDF::Term, context: &str) -> Result<RDF::Subject, ShaclParserError> {
    RDF::term_as_subject(term).map_err(|_| ShaclParserError::ExpectedSubject {
        term: term.to_string(),
        context: context.to_string(),
    })
}

fn shape<RDF: NeighsRDF + 'static>() -> impl RDFNodeParse<RDF, Output = ASTShape> {
    node_shape()
        .then(move |ns| SuccessParser::new(ASTShape::NodeShape(Box::new(ns))))
        .or(property_shape().then(|ps| SuccessParser::new(ASTShape::PropertyShape(Box::new(ps)))))
}
