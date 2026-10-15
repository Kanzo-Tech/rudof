use std::{fmt::Display, str::FromStr};

use rudof_iri::IriS;
use rudof_iri::error::IriSError;
use serde::{Deserialize, Serialize};

use prefixmap::error::DerefError;
use prefixmap::{DerefIri, IriRef};
use thiserror::Error;

use crate::ir::shape_label::ShapeLabel;

use super::bnode::BNode;

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq, Hash, Clone)]
#[serde(try_from = "&str", into = "String")]
pub enum ShapeExprLabel {
    IriRef { value: IriRef },
    BNode { value: BNode },
    Start,
}

impl ShapeExprLabel {
    pub fn iri_unchecked(s: &str) -> Self {
        ShapeExprLabel::IriRef {
            value: IriS::new_unchecked(s).into(),
        }
    }

    pub fn iri_ref(i: IriRef) -> Self {
        ShapeExprLabel::IriRef { value: i }
    }

    pub fn iri(iri: IriS) -> Self {
        ShapeExprLabel::IriRef {
            value: IriRef::iri(iri),
        }
    }

    pub fn bnode(bn: BNode) -> Self {
        ShapeExprLabel::BNode { value: bn }
    }

    pub fn prefixed(alias: &str, local: &str) -> Self {
        ShapeExprLabel::IriRef {
            value: IriRef::prefixed(alias, local),
        }
    }

    pub fn start() -> Self {
        ShapeExprLabel::Start
    }
}

impl DerefIri for ShapeExprLabel {
    fn deref_iri(self, base: Option<&IriS>, prefixmap: Option<&prefixmap::PrefixMap>) -> Result<Self, DerefError>
    where
        Self: Sized,
    {
        match self {
            ShapeExprLabel::IriRef { value } => {
                let new_value = value.deref_iri(base, prefixmap)?;
                Ok(ShapeExprLabel::IriRef { value: new_value })
            },
            ShapeExprLabel::BNode { value } => Ok(ShapeExprLabel::BNode { value: value.clone() }),
            ShapeExprLabel::Start => Ok(ShapeExprLabel::Start),
        }
    }
}

impl TryFrom<&str> for ShapeExprLabel {
    type Error = RefError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if let Some(iri_str) = angle_bracketed(s) {
            let iri_s = IriS::from_str(iri_str)?;
            Ok(ShapeExprLabel::IriRef {
                value: IriRef::iri(iri_s),
            })
        } else if let Some(bnode_s) = bnode_label(s) {
            Ok(ShapeExprLabel::BNode {
                value: BNode::new(bnode_s),
            })
        } else {
            let iri_s = IriS::from_str(s)?;
            Ok(ShapeExprLabel::IriRef {
                value: IriRef::iri(iri_s),
            })
        }
    }
}

/// What the regex `<(.*)>` captured: from the first `<` that has a `>` after it on its line, up to
/// the last `>` of that line (`.` does not cross `\n`, `*` is greedy, the search is unanchored).
/// Written out because this is the only thing that kept `regex` (its automata, its Unicode tables)
/// in a wasm build that only decodes a shape document.
fn angle_bracketed(s: &str) -> Option<&str> {
    s.match_indices('<').find_map(|(i, _)| {
        let rest = &s[i + 1..];
        let line = rest.split('\n').next().unwrap_or(rest);
        line.rfind('>').map(|j| &line[..j])
    })
}

/// What the regex `_:(.*)` captured: the rest of the line after the first `_:`.
fn bnode_label(s: &str) -> Option<&str> {
    let rest = &s[s.find("_:")? + 2..];
    Some(rest.split('\n').next().unwrap_or(rest))
}

#[cfg(test)]
mod label_tests {
    use super::{angle_bracketed, bnode_label};
    use regex::Regex;

    /// The two helpers answer exactly what the regexes they replaced answered, on the shapes of
    /// string a label comes in and on the edges (several brackets, newlines, nothing to capture).
    #[test]
    fn helpers_agree_with_the_regexes_they_replaced() {
        let re_iri = Regex::new(r"<(.*)>").unwrap();
        let re_bnode = Regex::new(r"_:(.*)").unwrap();
        for s in [
            "",
            "<",
            ">",
            "<>",
            "<a>",
            "<http://a/b>",
            "http://a/b",
            "_:b0",
            "_:",
            "x_:y",
            "<<a>>",
            "<a><b>",
            "<a> <b>",
            "a>b<c",
            "a<b\n>c",
            "<a\n>",
            "<a>\n<b>",
            "x\n<b>",
            "<a\n<b>",
            "_:a\nb",
            "\n_:a",
            "<_:a>",
            "_:<a>",
            "é<ü>é",
            "<a>>",
            ">< <a",
        ] {
            assert_eq!(
                angle_bracketed(s),
                re_iri.captures(s).map(|c| c.get(1).unwrap().as_str()),
                "iri {s:?}"
            );
            assert_eq!(
                bnode_label(s),
                re_bnode.captures(s).map(|c| c.get(1).unwrap().as_str()),
                "bnode {s:?}"
            );
        }
    }
}

impl Display for ShapeExprLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            ShapeExprLabel::IriRef { value } => value.to_string(),
            ShapeExprLabel::BNode { value } => value.to_string(),
            ShapeExprLabel::Start => "START".to_string(),
        };
        write!(f, "{str}")
    }
}

impl From<ShapeExprLabel> for String {
    fn from(val: ShapeExprLabel) -> Self {
        val.to_string()
    }
}

impl From<&ShapeExprLabel> for String {
    fn from(val: &ShapeExprLabel) -> Self {
        val.to_string()
    }
}

impl FromStr for ShapeExprLabel {
    type Err = RefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TryFrom::try_from(s)
    }
}

impl From<&ShapeLabel> for ShapeExprLabel {
    fn from(label: &ShapeLabel) -> Self {
        match label {
            ShapeLabel::Iri(iri) => ShapeExprLabel::IriRef {
                value: IriRef::iri(iri.clone()),
            },
            ShapeLabel::BNode(bnode) => ShapeExprLabel::BNode { value: bnode.clone() },
            ShapeLabel::Start => ShapeExprLabel::Start,
        }
    }
}

#[derive(Error, Debug)]
pub enum RefError {
    #[error("Cannot parse as IriS")]
    IriSError(#[from] IriSError),

    #[error("Cannot parse as Iri or BNode: {str}")]
    BadRef { str: String },
}
