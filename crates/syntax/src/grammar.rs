use tree_sitter::{Language, Query, QueryError};

/// Per-language tree-sitter parsing artifacts. Built once at first use by
/// [`crate::SyntaxRegistry`] and shared (via `Arc`) across every buffer that
/// uses this language. Parsing the highlights query is the slow part — we pay
/// it on first request and cache the result for the rest of the session.
///
/// `injections_query` is optional: grammars that don't embed other languages
/// (Rust, JSON, …) leave it `None`. When present, the snapshot pipeline runs
/// it after the main parse to discover regions to re-parse with another
/// grammar (`<script>` content as JS, fenced markdown blocks as their named
/// language, etc.).
///
/// `locals_query` describes scope/definition/reference structure and is used
/// (eventually) to honor `(#is-not? local)` predicates in highlights queries
/// — e.g. so a user-shadowed `console` doesn't render as a builtin. Compiled
/// up-front so the data is on hand when the consumer side lands; not
/// currently read by [`crate::SyntaxSnapshot::highlights`].
pub struct Grammar {
    pub language: Language,
    pub highlights_query: Query,
    pub injections_query: Option<Query>,
    pub locals_query: Option<Query>,
}

impl Grammar {
    pub fn new(language: Language, highlights_source: &str) -> Result<Self, QueryError> {
        let highlights_query = Query::new(&language, highlights_source)?;
        Ok(Self {
            language,
            highlights_query,
            injections_query: None,
            locals_query: None,
        })
    }

    pub fn with_injections(mut self, source: &str) -> Result<Self, QueryError> {
        self.injections_query = Some(Query::new(&self.language, source)?);
        Ok(self)
    }

    pub fn with_locals(mut self, source: &str) -> Result<Self, QueryError> {
        self.locals_query = Some(Query::new(&self.language, source)?);
        Ok(self)
    }
}
