use crate::RDFError;
use crate::term::IriOrBlankNode;
use crate::term::Triple;
use crate::term::literal::{ConcreteLiteral, Lang, NumericLiteral};
use prefixmap::{IriRef, PrefixMap, Show};
use rudof_iri::IriS;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{Debug, Display};

/// Represents an RDF object value in the object position of a triple.
///
/// In RDF, the object is the third component of a triple (subject-predicate-object)
/// and can be one of four types:
/// - **IRI**: A resource identified by an Internationalized Resource Identifier
/// - **Blank Node**: An anonymous resource without a global identifier
/// - **Literal**: A concrete value (string, number, date, etc.) with optional datatype/language
/// - **Triple** (RDF-star): A quoted triple that can be nested as an object
#[derive(Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Object {
    /// An IRI (Internationalized Resource Identifier) representing a named resource.
    Iri(IriS),
    /// A blank node (anonymous resource) identified by a local label.
    BlankNode(String),
    /// A literal value with a datatype and optional language tag.
    Literal(ConcreteLiteral),
    /// An RDF-star quoted triple that can be used as an object.
    ///
    /// # Fields
    /// - `subject`: The subject of the nested triple (IRI or blank node)
    /// - `predicate`: The predicate of the nested triple (IRI)
    /// - `object`: The object of the nested triple (recursively an Object)
    Triple {
        subject: Box<IriOrBlankNode>,
        predicate: IriS,
        object: Box<Object>,
    },
}

/// ## Constructors methods
impl Object {
    /// Creates an IRI object from an `IriS` instance.
    ///
    /// # Parameters
    /// - `iri`: The IRI to wrap as an object
    pub fn iri(iri: IriS) -> Object {
        Object::Iri(iri)
    }

    /// Creates a blank node object from a string identifier.
    ///
    /// # Parameters
    /// - `str`: The blank node identifier
    pub fn bnode(str: String) -> Object {
        Object::BlankNode(str)
    }

    /// Creates a literal object from a concrete literal value.
    ///
    /// # Parameters
    /// - `lit`: The concrete literal to wrap as an object
    pub fn literal(lit: ConcreteLiteral) -> Object {
        Object::Literal(lit)
    }

    /// Creates a string literal object from a string slice.
    ///
    /// # Parameters
    /// - `str`: The string value for the literal
    pub fn str(str: &str) -> Object {
        Object::Literal(ConcreteLiteral::str(str))
    }

    /// Creates a boolean literal object.
    ///
    /// # Parameters
    /// - `b`: The boolean value
    pub fn boolean(b: bool) -> Object {
        Object::Literal(ConcreteLiteral::boolean(b))
    }
}

/// ## Accessors methods
impl Object {
    // Returns the length (in bytes) of this object's string representation.
    ///
    /// - For IRIs: the length of the IRI string
    /// - For blank nodes: the length of the identifier
    /// - For literals: the length of the lexical form
    /// - For triples: the sum of all component lengths
    pub fn length(&self) -> usize {
        match self {
            Object::Iri(iri) => iri.as_str().len(),
            Object::BlankNode(bn) => bn.len(),
            Object::Literal(lit) => lit.lexical_form().len(),
            Object::Triple {
                subject,
                predicate,
                object,
            } => subject.as_ref().length() + predicate.as_str().len() + object.as_ref().length(),
        }
    }

    /// Extracts the numeric value if this is a numeric literal.
    ///
    /// # Returns
    /// - `Some(NumericLiteral)` if this is a numeric literal (integer, decimal, float, double)
    /// - `None` if this is not a literal or not a numeric type
    pub fn numeric_value(&self) -> Option<NumericLiteral> {
        match self {
            Object::Literal(lit) => lit.numeric_value(),
            _ => None,
        }
    }

    /// Returns the datatype IRI of this object if it's a literal.
    /// # Returns
    /// - `Some(IriRef)` if this is a literal
    /// - `None` if this is an IRI, blank node, or triple
    pub fn datatype(&self) -> Option<IriRef> {
        match self {
            Object::Literal(lit) => Some(lit.datatype()),
            _ => None,
        }
    }

    /// Returns the language tag if this is a language-tagged string literal.
    ///
    /// # Returns
    /// - `Some(&Lang)` if this is a string literal with a language tag (e.g., "en", "es-MX")
    /// - `None` if this is not a language-tagged literal
    pub fn lang(&self) -> Option<&Lang> {
        match self {
            Object::Literal(ConcreteLiteral::StringLiteral { lang: Some(lang), .. }) => Some(lang),
            _ => None,
        }
    }
}

impl Object {
    /// ## Parsing methods
    /// Parses a string into an RDF object, with optional base IRI resolution.
    ///
    /// This method attempts to parse:
    /// - Blank nodes: strings starting with "_:"
    /// - Literals: strings starting with '"' (not supported here)
    /// - IRIs: all other strings, resolved against the base if provided
    ///
    /// # Parameters
    /// - `str`: The string to parse
    /// - `base`: Optional base IRI for resolving relative IRI references
    ///
    /// # Errors
    /// - `RDFError::ParsingIri` if IRI parsing fails
    /// - `RDFError::ParseFailError` if given a quoted literal (use the RDF parser
    ///   for full N-Triples literal syntax, including datatypes and language tags)
    pub fn parse(str: &str, base: Option<&str>) -> Result<Object, RDFError> {
        if let Some(bnode_id) = str.strip_prefix("_:") {
            Ok(Object::bnode(bnode_id.to_string()))
        } else if str.starts_with('"') {
            Err(RDFError::ParseFailError {
                msg: format!(
                    "Parsing a quoted literal {str} as an Object is not supported here; use the RDF parser for full literal syntax"
                ),
            })
        } else {
            let iri = IriS::from_str_base(str, base).map_err(|e| RDFError::ParsingIri {
                iri: str.to_string(),
                error: e.to_string(),
            })?;
            Ok(Object::iri(iri))
        }
    }
}

/// ## Formatting methods
impl Object {
    /// Formats this object using qualified names (prefixes) where possible.
    ///
    /// This method produces a compact representation by replacing full IRIs
    /// with prefixed names (e.g., "rdf:type" instead of "http://www.w3.org/1999/02/22-rdf-syntax-ns#type").
    ///
    /// # Parameters
    /// - `prefixmap`: A prefix map containing IRI-to-prefix mappings
    pub fn show_qualified(&self, prefixmap: &prefixmap::PrefixMap) -> String {
        match self {
            Object::Iri(iri) => prefixmap.qualify(iri),
            Object::BlankNode(bnode) => format!("_:{bnode}"),
            Object::Literal(lit) => lit.show_qualified(prefixmap),
            Object::Triple {
                subject,
                predicate,
                object,
            } => format!(
                "<< {} {} {} >>",
                subject.show_qualified(prefixmap),
                prefixmap.qualify(predicate),
                object.show_qualified(prefixmap)
            ),
        }
    }
}

// ============================================================================
// Trait Implementations - Conversions
// ============================================================================

/// Converts an `IriS` into an `Object::Iri`.
///
/// This allows IRIs to be seamlessly used where objects are expected.
impl From<IriS> for Object {
    fn from(iri: IriS) -> Self {
        Object::Iri(iri)
    }
}

/// Converts a `ConcreteLiteral` into an `Object::Literal`.
///
/// This allows literals to be seamlessly used where objects are expected.
impl From<ConcreteLiteral> for Object {
    fn from(lit: ConcreteLiteral) -> Self {
        Object::Literal(lit)
    }
}

/// Converts an `Object` into an `oxrdf::Term`.
///
/// This enables interoperability with the `oxrdf` library by converting
/// the custom `Object` representation into oxrdf's term type.
impl From<Object> for oxrdf::Term {
    fn from(value: Object) -> Self {
        match value {
            Object::Iri(iri_s) => oxrdf::NamedNode::new_unchecked(iri_s.as_str()).into(),
            Object::BlankNode(bnode) => oxrdf::BlankNode::new_unchecked(bnode).into(),
            Object::Literal(literal) => oxrdf::Term::Literal(literal.into()),
            // RDF-star: recursively lower the quoted triple into an oxrdf quoted triple term.
            Object::Triple {
                subject,
                predicate,
                object,
            } => {
                let subj: oxrdf::NamedOrBlankNode = (*subject).into();
                let pred = oxrdf::NamedNode::new_unchecked(predicate.as_str());
                let obj: oxrdf::Term = (*object).into();
                oxrdf::Term::Triple(Box::new(oxrdf::Triple::new(subj, pred, obj)))
            },
        }
    }
}

/// Attempts to convert an `oxrdf::Term` into an `Object`.
///
/// This enables interoperability with the `oxrdf` library by converting
/// oxrdf's term type into the custom `Object` representation.
/// # Errors
/// Returns `RDFError` if the conversion fails (e.g., invalid literal format).
impl TryFrom<oxrdf::Term> for Object {
    type Error = RDFError;

    fn try_from(value: oxrdf::Term) -> Result<Self, Self::Error> {
        match value {
            oxrdf::Term::NamedNode(named_node) => Ok(Object::iri(named_node.into())),
            oxrdf::Term::BlankNode(blank_node) => Ok(Object::bnode(blank_node.into_string())),
            oxrdf::Term::Literal(literal) => {
                let lit: ConcreteLiteral = literal.try_into()?;
                Ok(Object::literal(lit))
            },
            oxrdf::Term::Triple(triple) => {
                let (s, p, o) = triple.into_components();
                let object = Object::try_from(o)?;
                let subject = IriOrBlankNode::from(s);
                let predicate = p.into();
                Ok(Object::Triple {
                    subject: Box::new(subject),
                    predicate,
                    object: Box::new(object),
                })
            },
        }
    }
}

/// Attempts to convert an `Object` into an `oxrdf::NamedOrBlankNode`.
///
/// This conversion is used when an object appears in subject position
/// (which can only be IRIs or blank nodes, not literals).
///
/// # Errors
/// Returns `RDFError` for objects that cannot be subjects (literals and triples).
impl TryFrom<Object> for oxrdf::NamedOrBlankNode {
    type Error = RDFError;

    fn try_from(value: Object) -> Result<Self, Self::Error> {
        match value {
            Object::Iri(iri_s) => Ok(oxrdf::NamedNode::new_unchecked(iri_s.as_str()).into()),
            Object::BlankNode(bnode) => Ok(oxrdf::BlankNode::new_unchecked(bnode).into()),
            // A literal cannot appear in subject position.
            Object::Literal(lit) => Err(RDFError::ExpectedIriOrBlankNodeFoundLiteral {
                literal: lit.to_string(),
            }),
            // oxrdf's NamedOrBlankNode cannot represent an RDF-star quoted triple subject.
            Object::Triple {
                subject,
                predicate,
                object,
            } => Err(RDFError::ExpectedIriOrBlankNodeFoundTriple {
                subject: subject.to_string(),
                predicate: predicate.to_string(),
                object: object.to_string(),
            }),
        }
    }
}

// ============================================================================
// Trait Implementations - Default, Display, Debug
// ============================================================================
impl Default for Object {
    /// Provides a default `Object` value (empty IRI).
    fn default() -> Self {
        Object::Iri(IriS::default())
    }
}

impl Display for Object {
    /// Formats the object for display (human-readable output).
    ///
    /// - IRIs: displayed as-is
    /// - Blank nodes: prefixed with "_:"
    /// - Literals: uses the literal's Display implementation
    /// - Triples (RDF-star): quoted with `<< subject predicate object >>`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Object::Iri(iri) => write!(f, "{iri}"),
            Object::BlankNode(bnode) => write!(f, "_:{bnode}"),
            Object::Literal(lit) => write!(f, "{lit}"),
            Object::Triple {
                subject,
                predicate,
                object,
            } => write!(f, "<< {subject} {predicate} {object} >>"),
        }
    }
}

impl Debug for Object {
    /// Formats the object for debugging (verbose output with type information).
    ///
    /// Includes type tags for each variant:
    /// - "Iri {<iri>}"
    /// - "Bnode{<id>}"
    /// - "Literal{<value>}"
    /// - "Triple {<s>, <p>, <o>}"
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Object::Iri(iri) => write!(f, "Iri {{{iri:?}}}"),
            Object::BlankNode(bnode) => write!(f, "Bnode{{{bnode:?}}}"),
            Object::Literal(lit) => write!(f, "Literal{{{lit:?}}}"),
            Object::Triple {
                subject,
                predicate,
                object,
            } => write!(f, "Triple {{{subject:?}, {predicate:?}, {object:?}}}"),
        }
    }
}

// ============================================================================
// Trait Implementations - Ordering
// ============================================================================

impl Object {
    /// Compares two objects following SPARQL/RDF value semantics (a **partial** order).
    ///
    /// The ordering priority is: IRIs < Blank Nodes < Literals.
    /// Within each category, standard comparison applies:
    /// - IRIs: lexicographic ordering of IRI strings
    /// - Blank nodes: lexicographic ordering of identifiers
    /// - Literals: ordering defined by [`ConcreteLiteral::sparql_compare`]
    ///
    /// Returns `None` for combinations that are not comparable under these rules
    /// (notably any comparison involving an RDF-star quoted triple, or
    /// incomparable literals). For a *total* order suitable for sorting use the
    /// [`Ord`]/[`PartialOrd`] implementations instead.
    pub fn sparql_compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Object::Iri(a), Object::Iri(b)) => Some(a.cmp(b)),
            (Object::BlankNode(a), Object::BlankNode(b)) => Some(a.cmp(b)),
            (Object::Literal(a), Object::Literal(b)) => a.sparql_compare(b),
            (Object::Iri(_), _) => Some(Ordering::Less),
            (Object::BlankNode(_), Object::Iri(_)) => Some(Ordering::Greater),
            (Object::BlankNode(_), Object::Literal(_)) => Some(Ordering::Less),
            (Object::Literal(_), _) => Some(Ordering::Greater),
            (Object::BlankNode(_), Object::Triple { .. }) => None,
            (Object::Triple { .. }, Object::Iri(_)) => None,
            (Object::Triple { .. }, Object::BlankNode(_)) => None,
            (Object::Triple { .. }, Object::Literal(_)) => None,
            (Object::Triple { .. }, Object::Triple { .. }) => None,
        }
    }

    /// Returns a stable rank for each variant, used to define a total order.
    fn variant_rank(&self) -> u8 {
        match self {
            Object::Iri(_) => 0,
            Object::BlankNode(_) => 1,
            Object::Literal(_) => 2,
            Object::Triple { .. } => 3,
        }
    }
}

impl Ord for Object {
    /// Implements a **total** ordering for objects that never panics.
    ///
    /// Objects are ordered first by variant rank (IRI < BlankNode < Literal <
    /// Triple) and then by their contents. This is a structural total order; for
    /// SPARQL/RDF value semantics use [`Object::sparql_compare`].
    fn cmp(&self, other: &Self) -> Ordering {
        self.variant_rank().cmp(&other.variant_rank()).then_with(|| match (self, other) {
            (Object::Iri(a), Object::Iri(b)) => a.cmp(b),
            (Object::BlankNode(a), Object::BlankNode(b)) => a.cmp(b),
            (Object::Literal(a), Object::Literal(b)) => a.cmp(b),
            (
                Object::Triple {
                    subject: s1,
                    predicate: p1,
                    object: o1,
                },
                Object::Triple {
                    subject: s2,
                    predicate: p2,
                    object: o2,
                },
            ) => s1.cmp(s2).then_with(|| p1.cmp(p2)).then_with(|| o1.cmp(o2)),
            // Different variants are fully discriminated by `variant_rank` above.
            _ => Ordering::Equal,
        })
    }
}

impl PartialOrd for Object {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ============================================================================
// Trait Implementations - Other
// ============================================================================

impl Show for Object {
    fn show(&self, pm: &PrefixMap) -> String {
        match self {
            Object::Iri(iri) => pm.qualify(iri),
            Object::BlankNode(n) => format!("_:{n}"),
            Object::Literal(lit) => lit.to_string(),
            Object::Triple {
                subject,
                predicate,
                object,
            } => format!(
                "<<{} {} {}>>",
                pm.show(subject.as_ref()),
                pm.qualify(predicate),
                pm.show(object.as_ref())
            ),
        }
    }
}
