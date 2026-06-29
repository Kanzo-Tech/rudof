use crate::ir::ShapeLabelIdx;
use crate::validator::report::ValidationResult;
use rudof_rdf::rdf_core::term::Object;
use std::collections::HashMap;

/// Per-engine validation cache.
///
/// Owned `HashMap`, mutated via `&mut self` — no `Arc`, no locks, no interior
/// mutability. Each engine owns its cache; a forked engine (parallel task)
/// starts empty, and the sequential/wasm path threads a single engine so it
/// keeps full cross-shape memoization. Within a shape's own `sh:node`/`sh:or`
/// recursion the memo is always present (that is the only place correctness
/// needs it), so dropping the cross-thread *shared* cache costs at most some
/// redundant work on the parallel-native path, never correctness.
#[derive(Debug, Default)]
pub(crate) struct ValidationCache(HashMap<(Object, ShapeLabelIdx), Vec<ValidationResult>>);

impl ValidationCache {
    /// Record the validation results for a `(node, shape_idx)` pair.
    pub fn record(&mut self, node: Object, shape_idx: ShapeLabelIdx, results: Vec<ValidationResult>) {
        self.0.insert((node, shape_idx), results);
    }

    /// Returns `true` if `(node, shape_idx)` has already been validated.
    pub fn has_validated(&self, node: &Object, shape_idx: ShapeLabelIdx) -> bool {
        self.0.contains_key(&(node.clone(), shape_idx))
    }

    /// Borrows the cached results for `(node, shape_idx)`, if any. Returns a
    /// slice (there is no lock guard to tie a lifetime to anymore).
    pub fn get_results(&self, node: &Object, shape_idx: ShapeLabelIdx) -> Option<&[ValidationResult]> {
        self.0.get(&(node.clone(), shape_idx)).map(Vec::as_slice)
    }
}
