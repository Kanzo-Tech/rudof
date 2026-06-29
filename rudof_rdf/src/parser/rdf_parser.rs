use crate::{
    NeighsRDF, RDFError,
    parser::{
        ParseCtx, RdfFocus,
        rdf_node_parser::{
            RDFNodeParse,
            constructors::{
                HasTypeParser, InstancesParser, ListParser, SatisfyParser, SingleInstanceParser,
                SingleValuePropertyParser, TypeParser, ValuesPropertyParser,
            },
        },
    },
};
use prefixmap::PrefixMap;
use rudof_iri::IriS;
use std::collections::HashSet;

/// Execution context for RDF parsing operations.
///
/// Owns the RDF graph and a parser-owned [`RdfFocus`] cursor, handing out a borrowed
/// [`ParseCtx`] to run parsers against. The graph itself is never mutated by parsing —
/// only the focus cursor moves.
pub struct RDFParse<RDF>
where
    RDF: NeighsRDF,
{
    /// The underlying RDF graph.
    rdf: RDF,

    /// The parser-owned focus cursor.
    focus: RdfFocus<RDF>,
}

impl<RDF> RDFParse<RDF>
where
    RDF: NeighsRDF,
{
    /// Creates a new parsing context wrapping the RDF graph.
    pub fn new(rdf: RDF) -> Self {
        Self {
            rdf,
            focus: RdfFocus::empty(),
        }
    }

    /// Creates a context and immediately sets the focus to the given IRI.
    pub fn with_focus(rdf: RDF, focus_iri: &IriS) -> Self {
        let term: RDF::Term = focus_iri.clone().into();
        Self {
            rdf,
            focus: RdfFocus::new(term),
        }
    }

    // ============================================================================
    // Basic accessors
    // ============================================================================

    /// Returns the prefix map of the underlying graph.
    pub fn prefixmap(&self) -> Option<PrefixMap> {
        self.rdf.prefixmap()
    }

    /// Gets the current focus node, if set.
    pub fn current_focus(&self) -> Option<&RDF::Term> {
        self.focus.get()
    }

    /// Sets the focus node to a specific term.
    pub fn set_focus(&mut self, focus: &RDF::Term) {
        self.focus.set(focus.clone());
    }

    /// Sets the focus node from an IRI.
    pub fn set_focus_iri(&mut self, iri: &IriS) {
        let term: RDF::Term = iri.clone().into();
        self.focus.set(term);
    }

    /// Returns a reference to the underlying RDF graph.
    pub fn rdf(&self) -> &RDF {
        &self.rdf
    }

    /// Borrows the graph and focus cursor together as a [`ParseCtx`] for running parsers.
    pub fn ctx(&mut self) -> ParseCtx<'_, RDF> {
        ParseCtx::new(&self.rdf, &mut self.focus)
    }

    // ============================================================================
    // Generic execution (core)
    // ============================================================================

    /// Executes any parser against the current context.
    ///
    /// This is the universal entry point - accepts any parser implementing
    /// `RDFNodeParse`, from simple property extractors to complex compositions.
    pub fn run<P, T>(&mut self, parser: P) -> Result<T, RDFError>
    where
        P: RDFNodeParse<RDF, Output = T>,
    {
        let mut ctx = ParseCtx::new(&self.rdf, &mut self.focus);
        parser.parse_focused(&mut ctx)
    }

    /// Executes a parser starting from a specific node, restoring the previous focus after.
    pub fn run_from<P, T>(&mut self, start_node: &IriS, parser: P) -> Result<T, RDFError>
    where
        P: RDFNodeParse<RDF, Output = T>,
    {
        let previous = self.focus.get().cloned();
        let term: RDF::Term = start_node.clone().into();
        self.focus.set(term);

        let result = self.run(parser);

        if let Some(prev) = previous {
            self.focus.set(prev);
        }

        result
    }

    // ============================================================================
    // Convenience methods (wrappers around common constructors)
    // ============================================================================

    /// Gets all values of a property from the current focus node.
    pub fn get_property_values(&mut self, pred: IriS) -> Result<HashSet<RDF::Term>, RDFError> {
        self.run(ValuesPropertyParser::new(pred))
    }

    /// Gets a single value of a property.
    pub fn get_property(&mut self, pred: IriS) -> Result<RDF::Term, RDFError> {
        self.run(SingleValuePropertyParser::new(pred))
    }

    /// Gets the `rdf:type` of the current focus node.
    pub fn get_type(&mut self) -> Result<RDF::Term, RDFError> {
        self.run(TypeParser::new())
    }

    /// Checks if the current focus has the given type.
    pub fn has_type(&mut self, expected: IriS) -> Result<(), RDFError> {
        self.run(HasTypeParser::new(expected))
    }

    /// Parses an RDF list starting at the current focus.
    pub fn parse_list(&mut self) -> Result<Vec<RDF::Term>, RDFError> {
        self.run(ListParser::new())
    }

    /// Parses an RDF list pointed to by a property.
    pub fn get_list_property(&mut self, pred: IriS) -> Result<Vec<RDF::Term>, RDFError> {
        let head = self.get_property(pred)?;
        self.focus.set(head);
        self.run(ListParser::new())
    }

    // ============================================================================
    // Graph-wide queries
    // ============================================================================

    /// Finds all instances of a given type in the entire graph (restores focus after).
    pub fn find_instances_of(&mut self, type_iri: IriS) -> Result<Vec<RDF::Subject>, RDFError> {
        let saved = self.focus.get().cloned();
        let result = self.run(InstancesParser::new(type_iri));
        if let Some(focus) = saved {
            self.focus.set(focus);
        }
        result
    }

    /// Finds exactly one instance of a type (fails if not exactly one); restores focus after.
    pub fn find_single_instance(&mut self, type_iri: IriS) -> Result<RDF::Subject, RDFError> {
        let saved = self.focus.get().cloned();
        let result = self.run(SingleInstanceParser::new(type_iri));
        if let Some(focus) = saved {
            self.focus.set(focus);
        }
        result
    }

    /// Validates current focus against a predicate.
    pub fn check<F>(&mut self, predicate: F, name: &str) -> Result<(), RDFError>
    where
        F: Fn(&RDF::Term) -> bool,
    {
        self.run(SatisfyParser::new(predicate, name))
    }
}
