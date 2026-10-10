use crate::ast::{ASTSchema, ASTShape};
use crate::error::ASTError;
use crate::ir::dg::{DependencyGraph, PosNeg};
use crate::ir::error::IRError;
use crate::ir::shape::IRShape;
use crate::ir::shape_label_idx::ShapeLabelIdx;
use crate::messages::MessageCatalog;
use crate::rdf::ShaclParser;
use prefixmap::PrefixMap;
use rudof_iri::IriS;
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::term::Object;
use rudof_rdf::vocab::{RdfVocab, RdfVocabulary, ShaclVocab, XsdVocab};
use rudof_rdf::{BuildRDF, RDFFormat};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::io::{Cursor, Read};
use tracing::warn;

#[derive(Clone, Debug)]
pub struct IRSchema {
    // imports: Vec<IriS>
    // entailments: Vec<IriS>
    labels_idx_map: HashMap<Object, ShapeLabelIdx>,

    shapes: HashMap<ShapeLabelIdx, IRShape>,
    prefixmap: PrefixMap,
    base: Option<IriS>,
    dependency_graph: DependencyGraph,
    shape_label_counter: usize,
    /// The wording of the results of shapes that declare no `sh:message`.
    messages: Cow<'static, MessageCatalog>,
}

impl IRSchema {
    pub fn new(prefixmap: PrefixMap) -> Self {
        Self {
            labels_idx_map: HashMap::new(),
            shapes: HashMap::new(),
            prefixmap,
            base: None,
            dependency_graph: DependencyGraph::new(),
            shape_label_counter: 0,
            messages: Cow::Borrowed(MessageCatalog::builtin()),
        }
    }

    pub fn from_reader<R: Read>(
        reader: &mut R,
        source_name: &str,
        format: &RDFFormat,
        base: Option<&str>,
        reader_mode: &ReaderMode,
    ) -> Result<Self, IRError> {
        let mut graph = OxigraphInMemory::new();
        graph.merge_from_reader(reader, source_name, format, base, reader_mode)?;
        let ast = ShaclParser::new(graph).parse()?;

        ast.try_into()
    }

    pub fn from_str(
        data: &str,
        format: &RDFFormat,
        base: Option<&str>,
        reader_mode: &ReaderMode,
    ) -> Result<Self, IRError> {
        Self::from_reader(&mut Cursor::new(data), "String", format, base, reader_mode)
    }

    pub fn with_base(mut self, base: Option<IriS>) -> Self {
        self.base = base;
        self
    }

    /// Replace the message catalog (the built-in one by default).
    pub fn with_messages(mut self, messages: MessageCatalog) -> Self {
        self.messages = Cow::Owned(messages);
        self
    }

    pub fn messages(&self) -> &MessageCatalog {
        &self.messages
    }

    pub fn prefix_map(&self) -> &PrefixMap {
        &self.prefixmap
    }

    pub fn base(&self) -> Option<&IriS> {
        self.base.as_ref()
    }

    /// The dependencies between shapes (`sh:node`, `sh:and`, `sh:property`, …).
    pub fn dependency_graph(&self) -> &DependencyGraph {
        &self.dependency_graph
    }

    pub fn get_shape_from_idx(&self, shape_idx: &ShapeLabelIdx) -> Option<&IRShape> {
        self.shapes.get(shape_idx)
    }

    pub fn get_shape_from_idx_e(&self, shape_idx: &ShapeLabelIdx) -> Result<&IRShape, IRError> {
        self.get_shape_from_idx(shape_idx)
            .ok_or(IRError::ShapeNotFound(*shape_idx))
    }

    pub fn get_shape(&self, sref: &Object) -> Option<&IRShape> {
        let idx = self.labels_idx_map.get(sref)?;
        self.shapes.get(idx)
    }

    /// Returns the `ShapeLabelIdx` for the given shape reference `Object`, if it exists.
    pub fn get_idx(&self, sref: &Object) -> Option<&ShapeLabelIdx> {
        self.labels_idx_map.get(sref)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Object, &IRShape)> {
        self.labels_idx_map.iter().filter_map(move |(node, label_idx)| match self.shapes.get(label_idx) {
            Some(shape) => Some((node, shape)),
            None => {
                // Arena invariant: every interned label has a shape. If it is ever
                // violated, skip the orphan entry instead of crashing the iterator.
                warn!("Internal invariant: shape label index {label_idx} for node {node} missing from shapes map; skipping");
                None
            },
        })
    }

    /// Iterate over all shapes that have at least one target.
    pub fn iter_with_targets(&self) -> impl Iterator<Item = (&Object, &IRShape)> {
        self.iter().filter(|(_, shape)| !shape.targets().is_empty())
    }

    /// The schema validating `shape` alone: it keeps every target it declares — class, implicit
    /// class, node, subjects-of, objects-of, where — and every other shape loses its own. Nothing
    /// else changes, so what `shape` reaches (its property shapes, `sh:node`, `sh:and`, a
    /// qualified value shape) is checked as before, and a shape the others alone targeted yields
    /// no check: the report holds the results the whole report holds for `shape`'s targets.
    pub fn targeting(mut self, shape: ShapeLabelIdx) -> Self {
        for (idx, ir) in self.shapes.iter_mut() {
            if *idx != shape {
                ir.clear_targets();
            }
        }
        self
    }
}

impl IRSchema {
    fn get_next_idx(&mut self) -> usize {
        let out = self.shape_label_counter;
        self.shape_label_counter += 1;
        out
    }

    pub fn register_shape(
        &mut self,
        id: &Object,
        shape: Option<&ASTShape>,
        ast: &ASTSchema,
    ) -> Result<ShapeLabelIdx, IRError> {
        let shape = match shape {
            None => ast.get_shape(id).ok_or::<ASTError>(id.clone().into())?,
            Some(shape) => shape,
        };

        match self.labels_idx_map.get(id) {
            None => {
                let label_idx = ShapeLabelIdx::new(self.get_next_idx());
                self.labels_idx_map.insert(id.clone(), label_idx);
                let compiled = IRShape::compile(shape, ast, self)?;
                self.shapes.insert(label_idx, compiled);
                Ok(label_idx)
            },
            Some(idx) => Ok(*idx),
        }
    }

    pub fn register_shapes(&mut self, ids: Vec<Object>, ast: &ASTSchema) -> Result<Vec<ShapeLabelIdx>, IRError> {
        ids.into_iter().map(|id| self.register_shape(&id, None, ast)).collect()
    }

    pub fn compile(ast: &ASTSchema) -> Result<Self, IRError> {
        let mut schema_ir = Self::new(ast.prefixmap().clone()).with_base(ast.base().cloned());

        // The shapes take their indexes in the order of their labels, not of
        // the map they are kept in, which is another on every run: the plans
        // read them in that order, so one shapes graph compiles to one plan.
        let mut shapes: Vec<_> = ast.iter().collect();
        shapes.sort_unstable_by_key(|(id, _)| *id);
        for (id, shape) in shapes {
            schema_ir.register_shape(id, Some(shape), ast)?;
        }

        schema_ir.build_dependency_graph();

        if schema_ir.dependency_graph.has_cycles() {
            warn!(
                "The dependency graph has cycles. This is known as a recursive schema and the SHACL semantics for these schemas is implementation dependent"
            );
            warn!(
                "More information about recursive schemas can be found at https://www.w3.org/TR/shacl/#shapes-recursion"
            );
        }

        if schema_ir.dependency_graph.has_neg_cycle() {
            warn!(
                "Warning: The dependency graph has negative cycles. This may lead to unexpected behavior in SHACL validation due to non-stratified negation"
            );
        }

        Ok(schema_ir)
    }

    pub(crate) fn build_dependency_graph(&mut self) {
        let mut dg = DependencyGraph::new();
        let mut cache = HashSet::new();

        let mut shapes: Vec<_> = self.shapes.iter().collect();
        shapes.sort_unstable_by_key(|(idx, _)| **idx);
        for (idx, shape) in shapes {
            // Add edges, we start by positive edges, but the direction can change when there is some negation
            shape.add_edges(*idx, &mut dg, PosNeg::Pos, self, &mut cache);
        }

        self.dependency_graph = dg;
    }
}

impl TryFrom<ASTSchema> for IRSchema {
    type Error = IRError;

    fn try_from(value: ASTSchema) -> Result<Self, Self::Error> {
        IRSchema::compile(&value)
    }
}

impl TryFrom<&ASTSchema> for IRSchema {
    type Error = IRError;

    fn try_from(value: &ASTSchema) -> Result<Self, Self::Error> {
        IRSchema::compile(value)
    }
}

impl IRSchema {
    // TODO - Maybe change error type to IRerror
    pub fn build_graph<RDF: BuildRDF>(&self) -> Result<RDF, IRError> {
        let mut graph = RDF::empty();

        graph
            .set_prefix_map(self.prefixmap.clone())
            .map_err(|e| IRError::from_rdf_err::<RDF>("set prefix map", e))?;
        graph
            .add_prefix("rdf", RdfVocab::base_iri())
            .map_err(|e| IRError::from_rdf_err::<RDF>("add prefix rdf", e))?;
        graph
            .add_prefix("xsd", XsdVocab::base_iri())
            .map_err(|e| IRError::from_rdf_err::<RDF>("add prefix xsd", e))?;
        graph
            .add_prefix("sh", ShaclVocab::base_iri())
            .map_err(|e| IRError::from_rdf_err::<RDF>("add prefix sh", e))?;

        graph
            .add_base(&self.base().cloned())
            .map_err(|e| IRError::from_rdf_err::<RDF>("add base", e))?;

        self.labels_idx_map.iter().try_for_each(|(_, idx)| {
            let shape = self.shapes.get(idx).ok_or(IRError::ShapeNotFound(*idx))?;

            shape.register(&mut graph, &self.shapes)
        })?;

        Ok(graph)
    }
}

impl Display for IRSchema {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SHACL shapes graph IR")?;

        for (node, shape) in self.shapes.iter() {
            writeln!(f, "[{node}] -> {shape}")?;
        }
        writeln!(f, "Dependency graph: {}", self.dependency_graph)
    }
}
