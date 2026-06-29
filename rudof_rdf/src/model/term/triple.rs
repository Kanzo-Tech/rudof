use crate::Rdf;
use crate::term::{Iri, Subject, Term};
use std::fmt::{Debug, Display};

/// Represents an RDF triple.
///
/// An RDF triple consists of three components: a subject, a predicate, and an object.
///
/// # Type Parameters
///
/// * `S` - The subject type, which must implement `Subject`
/// * `P` - The predicate type, which must implement `Iri`
/// * `O` - The object type, which must implement `Term`
pub trait Triple<S, P, O>: Debug + Clone + Display
where
    S: Subject,
    P: Iri,
    O: Term,
{
    /// Constructs a new RDF triple from the given components.
    ///
    /// # Parameters
    ///
    /// * `subj` - The subject of the triple, convertible to type `S`
    /// * `pred` - The predicate of the triple, convertible to type `P`
    /// * `obj` - The object of the triple, convertible to type `O`
    fn new(subj: impl Into<S>, pred: impl Into<P>, obj: impl Into<O>) -> Self;

    /// Returns a reference to the subject of this triple.
    fn subj(&self) -> &S;

    /// Returns a reference to the predicate of this triple.
    fn pred(&self) -> &P;

    /// Returns a reference to the object of this triple.
    fn obj(&self) -> &O;

    /// Consumes the triple and returns its components as a tuple.
    ///
    /// This method takes ownership of the triple and returns `(subject, predicate, object)`,
    /// allowing you to extract the individual components without cloning.
    fn into_components(self) -> (S, P, O);

    /// Consumes the triple and returns only the subject.
    fn into_subject(self) -> S {
        self.into_components().0
    }

    /// Consumes the triple and returns only the predicate.
    fn into_predicate(self) -> P {
        self.into_components().1
    }

    /// Consumes the triple and returns only the object.
    fn into_object(self) -> O {
        self.into_components().2
    }
}

/// A concrete implementation of an RDF triple for a specific RDF model.
///
/// # Type Parameters
///
/// * `R` - The RDF implementation type that defines the specific types for subjects, predicates, and objects through its associated types
pub struct ConcreteTriple<R>
where
    R: Rdf,
{
    subj: R::Subject,
    pred: R::IRI,
    obj: R::Term,
}

impl<R> ConcreteTriple<R>
where
    R: Rdf,
{
    /// Creates a new concrete triple from owned components.
    ///
    /// # Parameters
    ///
    /// * `subj` - The subject component from the RDF model `R`
    /// * `pred` - The predicate component from the RDF model `R`
    /// * `obj` - The object component from the RDF model `R`
    pub fn new(subj: R::Subject, pred: R::IRI, obj: R::Term) -> Self {
        ConcreteTriple { subj, pred, obj }
    }

    /// Returns a reference to the subject component.
    pub fn subj(&self) -> &R::Subject {
        &self.subj
    }

    /// Returns a reference to the predicate component.
    pub fn pred(&self) -> &R::IRI {
        &self.pred
    }

    /// Returns a reference to the object component.
    pub fn obj(&self) -> &R::Term {
        &self.obj
    }

    /// Converts this triple from one RDF implementation to another.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The target RDF implementation type
    ///
    /// # Trait Bounds
    ///
    /// Requires that the target RDF model's types can be converted from the
    /// source RDF model's types:
    /// - `T::Subject: From<R::Subject>` - Subject conversion
    /// - `T::Term: From<R::Term>` - Term conversion
    /// - `T::IRI: From<R::IRI>` - IRI conversion
    pub fn cnv<T: Rdf>(self) -> ConcreteTriple<T>
    where
        T::Subject: From<R::Subject>,
        T::Term: From<R::Term>,
        T::IRI: From<R::IRI>,
    {
        ConcreteTriple {
            subj: T::Subject::from(self.subj),
            pred: T::IRI::from(self.pred),
            obj: T::Term::from(self.obj),
        }
    }
}

// ============================================================================
// Trait Implementations - Display
// ============================================================================

impl<R> Display for ConcreteTriple<R>
where
    R: Rdf,
{
    /// Formats the triple as a string.
    ///
    /// # Parameters
    ///
    /// * `f` - The formatter to write to
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{},{},{}>", self.subj, self.pred, self.obj)
    }
}
