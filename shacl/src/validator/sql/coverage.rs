//! What the SQL engine compiles, held against the SHACL Core list.
//!
//! One row per constraint component of SHACL Core (§4 of the
//! Recommendation), plus the features outside Core that the engine refuses.
//! The test below fails when a Core component is missing or not compiled,
//! and prints the table, so coverage is read from the code and not from prose.

/// How the engine treats a component or feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Compiled to SQL; the text says how.
    Compiled(&'static str),
    /// Refused when the plan is compiled; the text says why.
    Refused(&'static str),
}

/// `(sh: local name, coverage)`, in the order of the Recommendation.
pub const COVERAGE: &[(&str, Coverage)] = &[
    // 4.1 Value type
    (
        "ClassConstraintComponent",
        Coverage::Compiled("anti-join with the class extent (rdfs:subClassOf closure)"),
    ),
    (
        "DatatypeConstraintComponent",
        Coverage::Compiled("literal kind, datatype and lexical well-formedness"),
    ),
    ("NodeKindConstraintComponent", Coverage::Compiled("term kind")),
    // 4.2 Cardinality
    (
        "MinCountConstraintComponent",
        Coverage::Compiled("count per focus node, LEFT JOIN for none"),
    ),
    (
        "MaxCountConstraintComponent",
        Coverage::Compiled("count per focus node"),
    ),
    // 4.3 Value range
    (
        "MinExclusiveConstraintComponent",
        Coverage::Compiled("RDF term order (numeric, string, dateTime, boolean)"),
    ),
    ("MinInclusiveConstraintComponent", Coverage::Compiled("RDF term order")),
    ("MaxExclusiveConstraintComponent", Coverage::Compiled("RDF term order")),
    ("MaxInclusiveConstraintComponent", Coverage::Compiled("RDF term order")),
    // 4.4 String-based
    (
        "MinLengthConstraintComponent",
        Coverage::Compiled("character length of the lexical form"),
    ),
    (
        "MaxLengthConstraintComponent",
        Coverage::Compiled("character length of the lexical form"),
    ),
    (
        "PatternConstraintComponent",
        Coverage::Compiled("the dialect's regex, with sh:flags"),
    ),
    (
        "LanguageInConstraintComponent",
        Coverage::Compiled("language range match"),
    ),
    (
        "UniqueLangConstraintComponent",
        Coverage::Compiled("GROUP BY focus, language HAVING COUNT(*) > 1"),
    ),
    // 4.5 Property pair
    (
        "EqualsConstraintComponent",
        Coverage::Compiled("both anti-joins with the other predicate"),
    ),
    (
        "DisjointConstraintComponent",
        Coverage::Compiled("semi-join with the other predicate"),
    ),
    (
        "LessThanConstraintComponent",
        Coverage::Compiled("join with the other predicate, RDF term order"),
    ),
    (
        "LessThanOrEqualsConstraintComponent",
        Coverage::Compiled("join with the other predicate, RDF term order"),
    ),
    // 4.6 Logical
    ("NotConstraintComponent", Coverage::Compiled("not in fails(shape)")),
    ("AndConstraintComponent", Coverage::Compiled("in any fails(shape)")),
    ("OrConstraintComponent", Coverage::Compiled("in every fails(shape)")),
    (
        "XoneConstraintComponent",
        Coverage::Compiled("conforming-shape count <> 1"),
    ),
    // 4.7 Shape-based
    ("NodeConstraintComponent", Coverage::Compiled("in fails(shape)")),
    (
        "PropertyConstraintComponent",
        Coverage::Compiled("nested checks on the value nodes"),
    ),
    (
        "QualifiedMinCountConstraintComponent",
        Coverage::Compiled("count of conforming values per focus node"),
    ),
    (
        "QualifiedMaxCountConstraintComponent",
        Coverage::Compiled("count of conforming values per focus node"),
    ),
    // 4.8 Other
    (
        "ClosedConstraintComponent",
        Coverage::Compiled("triples of the value nodes outside the allowed predicates"),
    ),
    (
        "HasValueConstraintComponent",
        Coverage::Compiled("anti-join with the constant"),
    ),
    (
        "InConstraintComponent",
        Coverage::Compiled("membership in the constant list"),
    ),
    // Outside SHACL Core
    (
        "SPARQLConstraintComponent",
        Coverage::Refused("SHACL-SPARQL is outside SHACL Core"),
    ),
    (
        "IfConstraintComponent",
        Coverage::Compiled("SHACL 1.2: fails(then) where cond holds, fails(else) where not"),
    ),
    (
        "targetWhere",
        Coverage::Compiled("SHACL 1.2: the nodes of the data graph that do not fail the shape"),
    ),
    (
        "ReifierShapeConstraintComponent",
        Coverage::Refused("SHACL 1.2 reifier shapes, not compiled yet"),
    ),
    (
        "recursive shapes",
        Coverage::Refused("SHACL leaves their semantics undefined"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every constraint component of SHACL Core (§4), from the Recommendation.
    const SHACL_CORE: [&str; 29] = [
        "ClassConstraintComponent",
        "DatatypeConstraintComponent",
        "NodeKindConstraintComponent",
        "MinCountConstraintComponent",
        "MaxCountConstraintComponent",
        "MinExclusiveConstraintComponent",
        "MinInclusiveConstraintComponent",
        "MaxExclusiveConstraintComponent",
        "MaxInclusiveConstraintComponent",
        "MinLengthConstraintComponent",
        "MaxLengthConstraintComponent",
        "PatternConstraintComponent",
        "LanguageInConstraintComponent",
        "UniqueLangConstraintComponent",
        "EqualsConstraintComponent",
        "DisjointConstraintComponent",
        "LessThanConstraintComponent",
        "LessThanOrEqualsConstraintComponent",
        "NotConstraintComponent",
        "AndConstraintComponent",
        "OrConstraintComponent",
        "XoneConstraintComponent",
        "NodeConstraintComponent",
        "PropertyConstraintComponent",
        "QualifiedMinCountConstraintComponent",
        "QualifiedMaxCountConstraintComponent",
        "ClosedConstraintComponent",
        "HasValueConstraintComponent",
        "InConstraintComponent",
    ];

    #[test]
    fn every_shacl_core_component_compiles() {
        let mut table = String::from("| component | SQL engine |\n|---|---|\n");
        for (name, coverage) in COVERAGE {
            let cell = match coverage {
                Coverage::Compiled(how) => format!("compiled: {how}"),
                Coverage::Refused(why) => format!("refused: {why}"),
            };
            table.push_str(&format!("| sh:{name} | {cell} |\n"));
        }
        // Printed so `cargo test -- --nocapture` shows the coverage.
        println!("{table}");
        for component in SHACL_CORE {
            let found = COVERAGE.iter().find(|(name, _)| *name == component);
            assert!(
                matches!(found, Some((_, Coverage::Compiled(_)))),
                "sh:{component} is SHACL Core and must compile to SQL\n{table}"
            );
        }
    }
}
