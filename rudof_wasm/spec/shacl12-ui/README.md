# SHACL 1.2 UI scoring data

The W3C data behind the default-editor choice in `src/scoring.rs`, vendored **unmodified** so that no score is written by hand in this crate. Nothing here is edited: to update it, copy the files from a newer commit and change the pin below.

- Source: <https://github.com/w3c/data-shapes> (default branch `gh-pages`)
- Pinned commit: `d556a1fc90c5d5ffdf388124cbb687cf43c5a35e`, committed 2026-09-30 (`#1285: NOW() must always return the same result (#1286)`)
- Specification: SHACL 1.2 UI, Editor's Draft, <https://w3c.github.io/data-shapes/shacl12-ui/>; namespace `shui:` = `http://www.w3.org/ns/shacl-ui/` (with the slash)
- Licence: W3C Software and Document License, <http://www.w3.org/Consortium/Legal/copyright-software>, per the repository's `LICENSE.md` ("All documents in this Repository are licensed by contributors under the W3C Software and Document License")

The Editor's Draft publishes no single default scoring graph. The scoring graph is the union of the 26 widget files (16 editors, 10 viewers) and the matcher shapes in `widgets/score-shapes.ttl`. `shacl-ui.ttl` is the vocabulary; it supplies the class hierarchy (`shui:SingleEditor rdfs:subClassOf shui:Editor`) the score function needs to keep only editors.

| file (relative to the repository root, under `shacl12-ui/` unless noted) | sha256 |
|---|---|
| `shacl12-vocabularies/shacl-ui.ttl (here: shacl-ui.ttl)` | `3ae24e6f479a4f25d126e21ba1276f2b1b77d54bf7e6a7351248cb60bae91261` |
| `widgets/editors/auto-complete-editor.ttl` | `a83c9f314e1de69de800835fe047da02e0059dd78ed5785d5b4e71cd350921ce` |
| `widgets/editors/blank-node-editor.ttl` | `1270e7a9018203e05e7a5b2fdb4085fd05d77be054195f825ecb0c78596d2356` |
| `widgets/editors/boolean-editor.ttl` | `393b689da69e044ad08111e59f55173ad53d62b8e06fffd573e11770c67b552a` |
| `widgets/editors/date-picker-editor.ttl` | `ff0606bf21ae9e2e02fac08c7454e5ce54b4db6e60c3344e021d3939fcc8f314` |
| `widgets/editors/date-time-picker-editor.ttl` | `33309fcd66d1672c8481fbb94c583310369f50090d2f88ac50a18ad1a95e2898` |
| `widgets/editors/details-editor.ttl` | `45b902ac20610e72861314c187519721fe1d416ab5cb053c406f915abb6afd7c` |
| `widgets/editors/enum-select-editor.ttl` | `87c83eebfd6796102d1169e27c286d75414ba714e06c57b649ab1692efe7cb78` |
| `widgets/editors/instances-select-editor.ttl` | `0d47e54d64b38fe03175cd9f039874fa46cd20dc043a8280d6301804eaf6917c` |
| `widgets/editors/iri-editor.ttl` | `9ad54e3d5feb04366da2ba230bf90f443e72d456c384784798f23836c9a5bfd9` |
| `widgets/editors/number-field-editor.ttl` | `bdf39f5c267e4faf07d22f2913d01d7d633a25677af1a8e74f684541a3bcce0f` |
| `widgets/editors/rich-text-editor.ttl` | `067919df5d2f0987d8daacc4a9b2b6ead889450cf73f2beb87b4ce41e104a9c6` |
| `widgets/editors/sub-class-editor.ttl` | `e10120e847e2a4b78a98594484064dc1b4bf1188f1f895714338a5f812e9a768` |
| `widgets/editors/text-area-editor.ttl` | `88dfdf45c86bf902a958f92519f99f0636fedcc24e9a6f0cfabca48b90a26ab9` |
| `widgets/editors/text-area-with-lang-editor.ttl` | `0a38e2c11ccdde0a3119536ad33145ae7811f2ab08a5f91c6d19dbb44053b040` |
| `widgets/editors/text-field-editor.ttl` | `31a0d14be56336fd94737377b952992f9e13e121ee1d7918ad34f0d54c8cb10d` |
| `widgets/editors/text-field-with-lang-editor.ttl` | `a36f1e7829b5c053caf8c11a09f970b7f079223958257621957aa00adf6772da` |
| `widgets/score-shapes.ttl` | `a4d0226a7feb3efeb33ab13070619ddcc27d8fe1d552c4d537f22e686aebf3da` |
| `widgets/viewers/blank-node-viewer.ttl` | `5234f6485a8019c4a687fdec83d1f5bef5fbfd4afc37fec69f01284d1a520ebd` |
| `widgets/viewers/details-viewer.ttl` | `c336e7523fb2186c0b4b590c8fec2f667ef20481b00c0c8f3323d5ff10704429` |
| `widgets/viewers/html-viewer.ttl` | `eae99ced9b299abcc865246916af642082da50b00574af2d2f64cbf7e81f1d75` |
| `widgets/viewers/hyperlink-viewer.ttl` | `304ca15cdff5e37934d6e8b9735b8a695aa969b9fc88d5fefe9ec09271c2e005` |
| `widgets/viewers/image-viewer.ttl` | `5260446266678989d71f20d72e50874f79ce3425512146d800e3f069b944c7c2` |
| `widgets/viewers/iri-viewer.ttl` | `fca2773c22d67de1f27f6256216272e36c2414ba98e880c4c28a74d6d1b42b2a` |
| `widgets/viewers/label-viewer.ttl` | `b084bcba01604be91a2cb9607ef1ba32b2c198fddfb2e80b5a5c940ac3bd2cb1` |
| `widgets/viewers/lang-string-viewer.ttl` | `643ea7a9e937e2f12d37c88bc97b0c5f4cd34271a2b497e41ee2e9b06c7a22f6` |
| `widgets/viewers/literal-viewer.ttl` | `4fa0e9f7b65e815fd70bd8d0e3a78750bfbd70360eccedd95bc8c7687b1a1f57` |
| `widgets/viewers/value-table-viewer.ttl` | `17930d810c0c3b6219340d74046797e0a4aa7f5b242e4997f3830712ab51d359` |
