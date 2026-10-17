//! Flags QLever spatial-search `SERVICE`s that request a nearest-neighbor
//! search with the bounding-box algorithm.
//!
//! Mirrors the QLever warning (`SpatialJoin.cpp`):
//! "The bounding box spatial join algorithm does not support nearest neighbor
//! search. Using s2 geometry algorithm instead."
//!
//! QLever emits this at execution time, but it is fully determined by the
//! `SERVICE` configuration: `spatialSearch:algorithm spatialSearch:boundingBox`
//! together with `spatialSearch:numNearestNeighbors`.

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
    LazyLock::new(|| DiagnosticCode::String("bounding-box-nearest-neighbors".to_string()));

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

    fn spatial_search_query(algorithm: &str, limit: &str) -> String {
        formatdoc!(
            "PREFIX spatialSearch: <https://qlever.cs.uni-freiburg.de/spatialSearch/>
             SELECT * WHERE {{
               ?x <p> ?y .
               SERVICE spatialSearch: {{
                 _:config spatialSearch:algorithm {algorithm} ;
                          spatialSearch:left ?y ;
                          spatialSearch:right ?b ;
                          {limit} .
                 {{ ?a <p> ?b }}
               }}
             }}"
        )
    }

    #[test]
    fn triggered_by_bounding_box_with_nearest_neighbors() {
        let text = spatial_search_query(
            "spatialSearch:boundingBox",
            "spatialSearch:numNearestNeighbors 5",
        );
        let result = run(diagnostics, &text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(
            diagnostic.range,
            range_of(&text, "spatialSearch:boundingBox", 0)
        );
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn not_triggered_by_bounding_box_with_max_distance() {
        let result = run(
            diagnostics,
            &spatial_search_query(
                "spatialSearch:boundingBox",
                "spatialSearch:maxDistance 1000",
            ),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_s2_with_nearest_neighbors() {
        let result = run(
            diagnostics,
            &spatial_search_query("spatialSearch:s2", "spatialSearch:numNearestNeighbors 5"),
        );
        assert!(result.is_empty());
    }
}
