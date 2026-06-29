use super::SubsetRecorder;
use prefixmap::{PrefixMap, PrefixMapError};
use rudof_iri::IriS;
use rudof_rdf::rdf_core::{Matcher, NeighsRDF, Rdf};

/// A non-owning decorator over a [`NeighsRDF`] store that records the triples a
/// run actually touches.
///
/// `RecordingRdf` wraps `&R` and delegates every [`Rdf`]/[`NeighsRDF`]
/// operation to the inner store, overriding **only** [`NeighsRDF::triples_matching`]
/// to `.inspect` each yielded triple into the recorder. Because the override
/// uses `inspect`, recording stays **lazy**: triples are recorded as the
/// consumer advances the iterator, never eagerly materialized. Every other
/// `NeighsRDF` query has a default body that funnels through
/// `triples_matching`, so it is recorded automatically with no extra code.
///
/// This is a generic decorator over two type parameters (`R` and `Rec`) — there
/// is no `Box<dyn>` and no `Arc`; the recording path monomorphizes to the inner
/// store's exact iterator type.
pub struct RecordingRdf<'a, R: NeighsRDF, Rec: SubsetRecorder<R>> {
    inner: &'a R,
    rec: Rec,
}

impl<'a, R: NeighsRDF, Rec: SubsetRecorder<R>> RecordingRdf<'a, R, Rec> {
    /// Wraps an inner store with a recorder.
    pub fn new(inner: &'a R, rec: Rec) -> Self {
        Self { inner, rec }
    }

    /// Borrows the recorder (e.g. to inspect progress mid-run).
    pub fn recorder(&self) -> &Rec {
        &self.rec
    }

    /// Consumes the decorator and returns the recorder (which owns the
    /// recorded subgraph, for [`super::BuildRdfRecorder`]).
    pub fn into_recorder(self) -> Rec {
        self.rec
    }
}

impl<'a, R: NeighsRDF, Rec: SubsetRecorder<R>> Rdf for RecordingRdf<'a, R, Rec> {
    type Subject = R::Subject;
    type IRI = R::IRI;
    type Term = R::Term;
    type BNode = R::BNode;
    type Literal = R::Literal;
    type Triple = R::Triple;
    type Err = R::Err;

    fn qualify_iri(&self, iri: &Self::IRI) -> String {
        self.inner.qualify_iri(iri)
    }

    fn qualify_subject(&self, subj: &Self::Subject) -> String {
        self.inner.qualify_subject(subj)
    }

    fn qualify_term(&self, term: &Self::Term) -> String {
        self.inner.qualify_term(term)
    }

    fn prefixmap(&self) -> Option<PrefixMap> {
        self.inner.prefixmap()
    }

    fn resolve_prefix_local(&self, prefix: &str, local: &str) -> Result<IriS, PrefixMapError> {
        self.inner.resolve_prefix_local(prefix, local)
    }
}

impl<'a, R: NeighsRDF, Rec: SubsetRecorder<R>> NeighsRDF for RecordingRdf<'a, R, Rec> {
    fn triples(&self) -> Result<impl Iterator<Item = Self::Triple>, Self::Err> {
        // `triples()` is a distinct primitive (it is not routed through
        // `triples_matching`); delegate it unchanged.
        self.inner.triples()
    }

    fn triples_matching<Sb, P, O>(
        &self,
        subject: &Sb,
        predicate: &P,
        object: &O,
    ) -> Result<impl Iterator<Item = Self::Triple> + '_, Self::Err>
    where
        Sb: Matcher<Self::Subject>,
        P: Matcher<Self::IRI>,
        O: Matcher<Self::Term>,
    {
        let rec = &self.rec;
        // Lazy recording: each triple is handed to the recorder as the consumer
        // pulls it from the iterator — no intermediate allocation.
        Ok(self
            .inner
            .triples_matching(subject, predicate, object)?
            .inspect(move |triple| rec.record(triple)))
    }
}
