//! Flags payload variables of a QLever spatial-search `SERVICE` that are not
//! bound in its right child.
//!
//! Mirrors the QLever warning (`SpatialJoin.cpp`):
//! "Variable '?v' selected as payload to spatial join but not present in right
//! child."
//!
//! The right child is the group graph pattern inside the `SERVICE`. The
//! special payload `spatialSearch:all` selects every variable and is never
//! flagged.

use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode},
        textdocument::TextDocumentItem,
    },
};
use ll_sparql_parser::ast::Unit;
use std::sync::LazyLock;

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("invalid-spatial-join-payload".to_string()));

pub(super) fn diagnostics(
    _document: &TextDocumentItem,
    _ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    todo!()
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        lsp::diagnostic::DiagnosticSeverity,
        message_handler::diagnostic::test_utils::{range_of, run},
    };
    use indoc::formatdoc;
    use pretty_assertions::assert_eq;

    fn spatial_search_query(payload: &str) -> String {
        formatdoc!(
            "PREFIX spatialSearch: <https://qlever.cs.uni-freiburg.de/spatialSearch/>
             SELECT * WHERE {{
               ?x <p> ?y .
               SERVICE spatialSearch: {{
                 _:config spatialSearch:left ?y ;
                          spatialSearch:right ?b ;
                          spatialSearch:numNearestNeighbors 5 ;
                          spatialSearch:payload {payload} .
                 {{ ?a <p> ?b }}
               }}
             }}"
        )
    }

    #[test]
    fn triggered_by_variable_of_left_child() {
        let text = spatial_search_query("?x");
        let result = run(diagnostics, &text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(&text, "?x", 1));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_only_for_variable_missing_in_right_child() {
        let text = spatial_search_query("?a , ?nowhere");
        let result = run(diagnostics, &text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(&text, "?nowhere", 0));
    }

    #[test]
    fn not_triggered_by_variable_of_right_child() {
        let result = run(diagnostics, &spatial_search_query("?a"));
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_payload_all() {
        let result = run(diagnostics, &spatial_search_query("spatialSearch:all"));
        assert!(result.is_empty());
    }
}
