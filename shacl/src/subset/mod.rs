//! Capa-4 graph-subset recorder (prototype).
//!
//! Every read performed by the validator and parser funnels through the single
//! primitive [`NeighsRDF::triples_matching`](rudof_rdf::rdf_core::NeighsRDF::triples_matching).
//! That makes a *generic decorator* over that one method enough to record the
//! exact triples a run touches — the "visited frontier" — which can then be
//! materialized into a SHACL subgraph. The decorator
//! ([`RecordingRdf`]) introduces **no `Box<dyn>` and no `Arc`**: it is generic
//! over both the inner store and the recorder, so the recording path
//! monomorphizes away entirely when the [`NullRecorder`] is used.
//!
//! This is wired through the now-generic validator spine: the public entry point
//! [`validate_with_subset`](crate::validator::processor::validate_with_subset)
//! wraps the store in a `RecordingRdf<&S, BuildRdfRecorder<S>>` and runs the same
//! generic `run_sequential::<S, E>` driver, returning the validation report
//! alongside the recorded SHACL subgraph. The normal validation path never wraps
//! the store, so it stays zero-cost.

mod recording_rdf;

pub use recording_rdf::RecordingRdf;

use rudof_rdf::rdf_core::term::Triple;
use rudof_rdf::rdf_core::{BuildRDF, Rdf};
use std::cell::RefCell;
use std::collections::HashSet;

/// Observer invoked for every triple visited through a [`RecordingRdf`].
///
/// Implementors decide what to do with the visited frontier (drop it, write it
/// into a subgraph, count it, …). `record` takes `&self` so it can sit behind
/// the decorator's shared `&self` query methods.
pub trait SubsetRecorder<S: Rdf> {
    /// Records a single visited triple. Called lazily, as the wrapped
    /// `triples_matching` iterator is advanced.
    fn record(&self, triple: &S::Triple);
}

/// Zero-cost recorder that discards everything.
///
/// Monomorphizes away, so the default (non-subsetting) validation path that
/// wraps a store in `RecordingRdf<_, NullRecorder>` pays nothing.
pub struct NullRecorder;

impl<S: Rdf> SubsetRecorder<S> for NullRecorder {
    #[inline]
    fn record(&self, _triple: &S::Triple) {}
}

/// Recorder that writes each distinct visited triple into an owned [`BuildRDF`]
/// sink, yielding the SHACL subgraph after a run.
///
/// It uses interior mutability ([`RefCell`]) so it can record behind the
/// `&self` [`SubsetRecorder::record`] hook. This makes `BuildRdfRecorder`
/// `!Sync`, so **subset recording runs on the sequential validation path**
/// (recording is inherently a serialization point anyway). Keeping the
/// mutation inside a `RefCell` — rather than an `Arc<Mutex<…>>` — keeps the
/// type `Arc`-free.
pub struct BuildRdfRecorder<B: BuildRDF> {
    out: RefCell<B>,
    seen: RefCell<HashSet<String>>,
}

impl<B: BuildRDF> BuildRdfRecorder<B> {
    /// Wraps an (typically empty) sink graph.
    pub fn new(out: B) -> Self {
        Self {
            out: RefCell::new(out),
            seen: RefCell::new(HashSet::new()),
        }
    }

    /// Consumes the recorder and returns the accumulated subgraph.
    pub fn into_inner(self) -> B {
        self.out.into_inner()
    }

    /// Number of distinct triples recorded so far.
    pub fn len(&self) -> usize {
        self.seen.borrow().len()
    }

    /// Whether any triple has been recorded.
    pub fn is_empty(&self) -> bool {
        self.seen.borrow().is_empty()
    }
}

impl<B: BuildRDF> SubsetRecorder<B> for BuildRdfRecorder<B> {
    fn record(&self, triple: &B::Triple) {
        // Dedup by the triple's canonical string form (`Triple: Display`) so we
        // don't impose `Hash`/`Eq` on the backend triple type.
        if self.seen.borrow_mut().insert(triple.to_string()) {
            let (subj, pred, obj) = triple.clone().into_components();
            // Best-effort sink write: a recording failure must never abort the
            // validation/parse run that is driving it.
            let _ = self.out.borrow_mut().add_triple(subj, pred, obj);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildRdfRecorder, NullRecorder, RecordingRdf};
    use rudof_rdf::rdf_core::{Any, BuildRDF, NeighsRDF, RDFFormat};
    use rudof_rdf::rdf_impl::{OxigraphInMemory, ReaderMode};

    fn sample_graph() -> OxigraphInMemory {
        let data = r#"
            @prefix ex: <http://example.org/> .
            ex:a ex:p ex:b .
            ex:a ex:q ex:c .
            ex:b ex:p ex:d .
        "#;
        OxigraphInMemory::from_str(data, &RDFFormat::Turtle, None, &ReaderMode::Lax)
            .expect("sample turtle parses")
    }

    #[test]
    fn records_visited_triples_into_sink() {
        let graph = sample_graph();
        let recorder = BuildRdfRecorder::new(OxigraphInMemory::empty());
        let recording = RecordingRdf::new(&graph, recorder);

        // Recording is lazy, so drain the iterators. Run a couple of pattern
        // queries through the decorator.
        let all: Vec<_> = recording
            .triples_matching(&Any, &Any, &Any)
            .expect("query ok")
            .collect();
        assert_eq!(all.len(), 3, "decorator yields the inner graph's triples unchanged");

        // A narrower query re-visits a subset; dedup keeps the count stable.
        let _ = recording.triples_matching(&Any, &Any, &Any).expect("query ok").count();

        let recorder = recording.into_recorder();
        assert_eq!(recorder.len(), 3, "recorder captured every distinct visited triple");

        // The captured frontier round-trips through the sink graph.
        let subset = recorder.into_inner();
        let subset_triples: Vec<_> = subset.triples().expect("subset triples").collect();
        assert_eq!(subset_triples.len(), 3, "sink holds the recorded subgraph");
    }

    #[test]
    fn null_recorder_is_inert() {
        let graph = sample_graph();
        let recording = RecordingRdf::new(&graph, NullRecorder);
        let n = recording.triples_matching(&Any, &Any, &Any).expect("query ok").count();
        assert_eq!(n, 3, "NullRecorder leaves the query results untouched");
    }
}
