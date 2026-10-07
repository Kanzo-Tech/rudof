use oxiri::IriParseError;
use oxrdfio::RdfSyntaxError;
use prefixmap::PrefixMapError;
use rudof_iri::error::IriSError;
use std::io;
use std::io::Error as IOError;
use thiserror::Error;

/// Represents all possible errors that can occur when working with in-memory RDF graphs.
#[derive(Error, Debug)]
pub enum OxigraphInMemoryError {
    /// Error processing query results.
    ///
    /// # Fields
    /// - `msg`: Detailed description of the query result error
    #[error("Query result error: {msg}")]
    QueryResultError { msg: String },

    /// Error extending query solutions with additional data.
    ///
    /// # Fields
    /// - `query`: The query string being executed
    /// - `error`: Detailed description of the extension failure
    #[error("Error extending query solutions for query: {query}: {error}")]
    ExtendingQuerySolutionsError { query: String, error: String },

    /// Error parsing a query string.
    ///
    /// # Fields
    /// - `msg`: Detailed description of the parsing failure
    #[error("Parsing query error: {msg}")]
    ParsingQueryError { msg: String },

    /// Error executing a query.
    ///
    /// # Fields
    /// - `query`: The query string that failed to execute
    /// - `msg`: Detailed description of the execution failure
    #[error("Running query {query} error: {msg}")]
    RunningQueryError { query: String, msg: String },

    /// The data is not in the syntax of its format: the RDF parser's own error,
    /// which places itself in the text when the parser can
    /// ([`RdfSyntaxError::location`]: 0-based, columns in code points).
    ///
    /// # Fields
    /// - `source_name`: The name or path of the data source
    /// - `error`: The parser's error
    #[error("Error parsing RDF data from {source_name}: {error}")]
    Syntax { source_name: String, error: RdfSyntaxError },

    /// Error when an RDF serialization format is not supported by this backend.
    ///
    /// # Fields
    /// - `format`: The name of the unsupported serialization format
    #[error("Unsupported RDF serialization format: {format}")]
    UnsupportedFormat { format: String },

    /// Error parsing a base IRI.
    ///
    /// # Fields
    /// - `str`: The IRI string that failed to parse
    /// - `error`: Detailed description of the parsing failure
    #[error("Parsing base iri {str}: error: {error}")]
    BaseParseError { str: String, error: String },

    /// Error generating a blank node identifier.
    ///
    /// # Fields
    /// - `msg`: Detailed description of the blank node generation failure
    #[error("Blank node generation id: {msg}")]
    BlankNodeId { msg: String },

    /// Error reading data from a file path.
    ///
    /// # Fields
    /// - `path_name`: The path that failed to be read
    /// - `error`: The underlying I/O error
    #[error("Reading path {path_name:?} error: {error:?}")]
    ReadingPathError { path_name: String, error: io::Error },

    /// General I/O error.
    ///
    /// # Fields
    /// - `err`: The underlying I/O error
    #[error(transparent)]
    IOError {
        #[from]
        err: IOError,
    },

    /// Error parsing an IRI.
    ///
    /// # Fields
    /// - `err`: The underlying IRI parsing error
    #[error(transparent)]
    IriParseError {
        #[from]
        err: IriParseError,
    },

    /// Error related to IRI string operations.
    ///
    /// # Fields
    /// - `err`: The underlying IRI string error
    #[error(transparent)]
    IriSError {
        #[from]
        err: IriSError,
    },

    /// Error related to prefix map operations.
    ///
    /// # Fields
    /// - `err`: The underlying prefix map error
    #[error(transparent)]
    PrefixMapError {
        #[from]
        err: PrefixMapError,
    },

    /// Error coming from the embedded Oxigraph SPARQL store.
    #[cfg(feature = "sparql")]
    #[error(transparent)]
    StorageError {
        #[from]
        err: oxigraph::store::StorageError,
    },
}
