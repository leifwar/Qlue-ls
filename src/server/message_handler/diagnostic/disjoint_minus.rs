//! Flags `MINUS` clauses that share no variables with the patterns before
//! them in the same group graph pattern.
//!
//! Mirrors the QLever warning (`QueryPlanner.cpp`):
//! "A MINUS clause that shares no variables with the patterns before it has no
//! effect and is ignored. Use FILTER NOT EXISTS if you want to remove all
//! results whenever the pattern of the MINUS clause matches."
//!
//! A `MINUS` at the start of a group has no preceding variables and is always
//! flagged. Variables bound in a preceding `OPTIONAL` count, variables of a
//! preceding `MINUS` do not.

use super::bound_variables;
use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity},
        textdocument::{Range, TextDocumentItem},
    },
};
use ll_sparql_parser::ast::{AstNode, MinusGraphPattern, Unit};
use std::{collections::HashSet, iter::successors, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("disjoint-minus".to_string()));

const MESSAGE: &str = "A MINUS clause that shares no variables with the patterns before it has no effect and is ignored.\n\
    Use FILTER NOT EXISTS if you want to remove all results whenever the pattern of the MINUS clause matches.";

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = ast
        .syntax()
        .descendants()
        .filter_map(MinusGraphPattern::cast)
        .filter_map(|minus_graph_pattern| {
            let minus_variables: HashSet<String> = minus_graph_pattern
                .visible_variables()
                .into_iter()
                .map(|var| var.var_name())
                .collect();
            // INFO: The MINUS is wrapped in a `GraphPatternNotTriples`, whose siblings are the
            // elements of the enclosing group.
            let pattern = minus_graph_pattern.syntax().parent()?;
            let preceding_variables: HashSet<String> =
                successors(pattern.prev_sibling(), |node| node.prev_sibling())
                    .flat_map(bound_variables)
                    .map(|var| var.var_name())
                    .collect();
            minus_variables
                .is_disjoint(&preceding_variables)
                .then(|| Diagnostic {
                    range: Range::from_byte_offset_range(
                        minus_graph_pattern.syntax().text_range(),
                        &document.text,
                    )
                    .expect("MinusGraphPattern should be withing the document"),
                    severity: DiagnosticSeverity::Warning,
                    code: Some((*CODE).clone()),
                    source: Some("qlue-ls".to_string()),
                    message: MESSAGE.to_string(),
                    data: None,
                })
        })
        .collect();
    (!diagnostics.is_empty()).then_some(diagnostics)
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        lsp::diagnostic::DiagnosticSeverity,
        message_handler::diagnostic::test_utils::{range_of, run},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn triggered_by_minus_without_shared_variables() {
        let text = "SELECT * WHERE { ?a <is-a> ?b MINUS { ?x <is-a> ?y } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(
            diagnostic.range,
            range_of(text, "MINUS { ?x <is-a> ?y }", 0)
        );
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
    }

    #[test]
    fn triggered_by_minus_at_start_of_group() {
        let text = "SELECT * WHERE { MINUS { ?x <is-a> ?y } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "MINUS { ?x <is-a> ?y }", 0));
    }

    #[test]
    fn triggered_by_minus_before_shared_variables() {
        // NOTE: Only patterns before the MINUS count, `?a` is bound after it.
        let text = "SELECT * WHERE { MINUS { ?a <is-a> ?y } ?a <is-a> ?b }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "MINUS { ?a <is-a> ?y }", 0));
    }

    #[test]
    fn triggered_once_per_disjoint_minus() {
        // NOTE: The variables of the first MINUS (`?y`) do not connect the second.
        let text = "SELECT * WHERE { ?a <is-a> ?b MINUS { ?x <is-a> ?y } MINUS { ?z <is-a> ?y } }";
        let result = run(diagnostics, text);
        assert_eq!(
            result.iter().map(|d| d.range.clone()).collect::<Vec<_>>(),
            vec![
                range_of(text, "MINUS { ?x <is-a> ?y }", 0),
                range_of(text, "MINUS { ?z <is-a> ?y }", 0)
            ]
        );
    }

    #[test]
    fn not_triggered_by_shared_variable() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?a <is-a> ?b MINUS { ?a <is-a> ?y } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn triggered_despite_preceding_filter() {
        // NOTE: FILTER never binds variables.
        let text = "SELECT * WHERE { ?a <is-a> ?b FILTER (?x > 1) MINUS { ?x <is-a> ?y } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "MINUS { ?x <is-a> ?y }", 0));
    }

    #[test]
    fn not_triggered_by_variable_bound_in_optional() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?a <is-a> ?b OPTIONAL { ?a <p> ?c } MINUS { ?c <is-a> ?y } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_filter_not_exists() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?a <is-a> ?b FILTER NOT EXISTS { ?x <is-a> ?y } }",
        );
        assert!(result.is_empty());
    }
}
