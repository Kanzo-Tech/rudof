//! The document base rudof gives RDF that arrived as a bare string.
//!
//! Every loading path in this workspace resolves relative IRIs against a base
//! that is *always* present: the CLI's `--base-data` / `--base-shapes` when the
//! user supplies one, otherwise one derived from where the document came from
//! (`rudof_lib`'s `InputSpec::guess_base` — a `file://` URL for a path, the
//! endpoint URL for a URL, `stdin://` for stdin). `load_data` takes
//! `base: IriS`, not `Option<IriS>`, precisely because a parse with no base at
//! all rejects documents that RDF says are legal.
//!
//! A string has no location to derive a base from, so the workspace answers with
//! this synthetic one. It is the single definition of that answer, here beside
//! the parser so that every reader of a string takes it from one place: the
//! CLI, the wasm form façade, and a host that must refuse exactly what they
//! refuse.

/// Synthetic document base for RDF parsed out of an in-memory string.
///
/// Absolute (it has a scheme), so `oxiri` accepts it as a base and resolves
/// relative references against it; distinctive, so a resolved IRI is visibly
/// "this came from a string with no base of its own" rather than masquerading as
/// an `http://` document.
pub const STRING_BASE: &str = "string://";
