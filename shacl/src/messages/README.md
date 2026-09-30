# Default validation messages

When a shape declares no `sh:message`, the validator writes the `sh:resultMessage`
of each result itself, in every language of a catalog that is plain RDF
(`messages.en.ttl`, `messages.es.ttl`, `messages.ca.ttl`, read once with
`include_str!`). This page says what the standards fix and what this engine chose.

## What the standards say (normative)

Quotes are verbatim, from the Recommendation and the current drafts.

1. **SHACL 1.0, section 3.6.2.7, `sh:resultMessage`**
   (<https://www.w3.org/TR/shacl/#results-message>; unchanged in SHACL 1.2 Core,
   <https://www.w3.org/TR/shacl12-core/>):
   - "These values are produced by a validation engine based on the values of
     sh:message of the constraints in the shapes graph, see Declaring Messages for
     a Shape. In cases where a constraint does not have any values for sh:message
     in the shapes graph the SHACL processor MAY automatically generate other
     values for sh:resultMessage."
   - "While sh:resultMessage may have multiple values, there should not be two
     values with the same language tag."
   - SHACL 1.2 Core adds: "Messages declared using reification have precedence
     over those declared at the surrounding shape."
2. **SHACL 1.0, section 2.1.5, Declaring Messages for a Shape**: "If a shape has at
   least one value for sh:message in the shapes graph, then all validation results
   produced as a result of the shape will have exactly these messages as their
   value of sh:resultMessage, i.e. the values will be copied from the shapes graph
   into the results graph."
3. **SHACL-SPARQL, section 6 (Rec) and SHACL 1.2 SPARQL Extensions**
   (<https://www.w3.org/TR/shacl-sparql/>, <https://www.w3.org/TR/shacl12-sparql/>),
   on `sh:resultMessage`: "For SPARQL-based constraint components: The values of
   sh:message of the validator of the SPARQL-based constraint component. For
   SPARQL-based constraint components: The values of sh:message of the SPARQL-based
   constraint component. These message literals may include the names of any SELECT
   result variables via {?varName} or {$varName}. If the constraint is based on a
   SPARQL-based constraint component, then the component's parameter names can also
   be used. These {?varName} and {$varName} blocks SHOULD be replaced with suitable
   string representations of the values of said variables."
   Its non-normative example declares a core component this way:
   `sh:PatternConstraintComponent a sh:ConstraintComponent ; ... sh:validator ex:hasPattern . ex:hasPattern a sh:SPARQLAskValidator ; sh:message "Value does not match pattern {$pattern}"`.

So normative are: the processor MAY generate messages where the shape gives none;
what the shape gives is copied exactly; no two values share a language tag; and the
template syntax `{$name}` / `{?name}` with the SHOULD to substitute.

## What this engine chose

- To generate them from `sh:message` literals attached to the constraint
  **component** (`sh:MinCountConstraintComponent sh:message "..."@en`), which is
  what SHACL-SPARQL already lets a component declare, applied to the core
  components under the "MAY automatically generate" clause. Reading `sh:message` off
  a core component is the engine's convention, not a SHACL rule.
- The placeholders: named after the parameters the SHACL vocabulary declares for
  the component (`{$minCount}`, `{$datatype}`, `{$class}`, `{$nodeKind}`,
  `{$pattern}`, `{$minLength}`, `{$in}`, `{$hasValue}`, `{$languageIn}`, ...), plus
  `{$value}`, the value node, which SHACL-SPARQL also pre-binds. `{?x}` and `{$x}`
  are the same. A name the engine cannot fill is left as written. IRIs are written
  in prefixed form when the shapes graph declares the prefix, literals by their
  lexical form, lists comma-separated.
- The wording, and its Spanish and Catalan translations.
- `sh:ConstraintComponent` holds the message for a component the catalog does not
  name; with none there either, the result has no message (never a debug string).
- `MessageCatalog::load` (and `FormEngine::load_messages`, `Session.loadMessages`)
  adds Turtle to the catalog; per (component, language) the later document wins.
- **Not supported**: a `sh:message` on a reifier of the constraint's triple
  (SHACL 1.2), because the parser does not read reified constraints; substituting
  `{$var}` in an author's own `sh:message` or in a SPARQL-based constraint's;
  `{$PATH}` and `{$this}`.

## What other implementations do

- **TopBraid SHACL API** ships the same thing as data: `dash.ttl`
  (<https://raw.githubusercontent.com/TopQuadrant/shacl/master/src/main/resources/rdf/dash.ttl>)
  has, for example, `sh:message "Fewer than {$minCount} values"` on
  `sh:MinCountConstraintComponent`, `"Value does not match pattern \"{$pattern}\""`
  on `sh:PatternConstraintComponent`, `"Value does not have datatype {$datatype}"`,
  `"Value does not have class {$class}"`, `"Value is not in {$in}"`,
  `"Predicate {?path} is not allowed (closed shape)"`. English only, untagged, and
  the parameter names are the same as here.
- **pySHACL** hard-codes English in Python
  (<https://raw.githubusercontent.com/RDFLib/pySHACL/master/pyshacl/constraints/core/cardinality_constraints.py>):
  `"Less than {} values on {}->{}"`, `"More than {} values on {}"`.
- **Apache Jena SHACL** hard-codes English in Java
  (<https://github.com/apache/jena/tree/main/jena-shacl/src/main/java/org/apache/jena/shacl/engine/constraint>):
  `"Invalid cardinality: expected min "+minCount+": Got count = "+count`,
  `"Expected class :"+...`.
- This engine used to be of the second kind (`"MinCount(1) not satisfied"`).
