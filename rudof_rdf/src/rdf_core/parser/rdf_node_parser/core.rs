use crate::rdf_core::{NeighsRDF, RDFError, parser::ParseCtx};
use rudof_iri::IriS;

/// A trait for parsing RDF data.
///
/// Types implementing `RDFNodeParse` parse an RDF graph relative to a *focus node* held
/// by the parser-owned [`ParseCtx`] cursor (rather than inside the graph itself). The graph
/// only needs to implement [`NeighsRDF`]; the focus lives in [`ParseCtx`].
///
/// This trait provides a combinator-based parsing API inspired by parser combinator libraries,
/// allowing complex parsers to be built by composing simpler ones.
///
/// # Type Parameters
///
/// * `RDF` - The RDF graph type that implements [`NeighsRDF`]
pub trait RDFNodeParse<RDF>
where
    RDF: NeighsRDF,
{
    /// The type returned when parsing succeeds.
    type Output;

    /// Parses RDF data starting from the specified node.
    ///
    /// This is the main entry point for parsing. It sets the focus node of the context
    /// to `node` and then runs the parser implementation.
    ///
    /// # Arguments
    ///
    /// * `node` - The IRI of the node to set as the focus before parsing
    /// * `ctx` - The parsing context (borrowed graph + focus cursor)
    fn parse(&self, node: &IriS, ctx: &mut ParseCtx<'_, RDF>) -> Result<Self::Output, RDFError> {
        let focus = RDF::Term::from(RDF::IRI::from(node.clone()));
        ctx.set_focus(&focus);
        self.parse_focused(ctx)
    }

    /// The internal parsing implementation that operates on the current focus node.
    ///
    /// This method performs the actual parsing logic without modifying which node is focused.
    /// It is called by [`parse`](Self::parse) after the focus has been set.
    ///
    /// # Arguments
    ///
    /// * `rdf` - The parsing context with the focus already set
    fn parse_focused(&self, rdf: &mut ParseCtx<'_, RDF>) -> Result<Self::Output, RDFError>;
}
