//! Flags variables used in a `HAVING` expression that are neither bound in the
//! query body nor introduced by an alias in the `SELECT` clause.
//!
//! Mirrors the QLever warning (`ParsedQuery.cpp`):
//! "Variable ?v was used by HAVING in expression "<expr>", but is not defined
//! in the query body and also not bound inside the SELECT clause."
//!
//! The part about the `SELECT` clause is only part of the message if the query
//! has one (i.e. not for `CONSTRUCT`, `ASK` or `DESCRIBE`). Aliases of the
//! `GROUP BY` clause count as bound. Variables of the trailing `VALUES` clause
//! do not, since it is joined after `HAVING`. Variables that only occur inside
//! `EXISTS`/`NOT EXISTS` are not flagged.

use super::{
    query_scope::{QueryScope, used_variables},
    warning,
};
use crate::server::lsp::{
    diagnostic::{Diagnostic, DiagnosticCode},
    textdocument::TextDocumentItem,
};
use ll_sparql_parser::ast::AstNode;
use std::{collections::HashSet, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-having-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    scopes: &[QueryScope],
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = scopes
        .iter()
        .flat_map(|scope| {
            // NOTE: HAVING is evaluated before the trailing VALUES clause is joined,
            // so its variables are not visible here.
            let visible_variables: HashSet<String> = scope
                .body_variables()
                .iter()
                .chain(scope.group_by_aliases())
                .chain(scope.select_aliases())
                .cloned()
                .collect();
            let select_hint = scope.select_hint();
            scope
                .having_conditions()
                .into_iter()
                .flat_map(move |condition| {
                    used_variables(&condition)
                        .into_iter()
                        .filter(|var| !visible_variables.contains(&var.var_name()))
                        .map(|var| {
                            let message = format!(
                                "Variable {} was used by HAVING in expression \"{}\", \
                                 but is not defined in the query body{}.",
                                var.syntax().text(),
                                condition.text(),
                                select_hint
                            );
                            (var.syntax().clone(), message)
                        })
                        .collect::<Vec<_>>()
                })
        })
        .map(|(node, message)| warning(document, &node, &CODE, message))
        .collect();
    (!diagnostics.is_empty()).then_some(diagnostics)
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        lsp::diagnostic::DiagnosticSeverity,
        message_handler::diagnostic::test_utils::{range_of, run_scoped},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn triggered_by_unbound_variable() {
        let text = "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p HAVING (COUNT(?x) > 1)";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?x", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(diagnostic.message.contains("SELECT clause"));
    }

    #[test]
    fn triggered_in_query_without_select_clause() {
        let text = "CONSTRUCT { ?p ?p ?p } WHERE { ?s ?p ?o } GROUP BY ?p HAVING (COUNT(?x) > 1)";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
        assert!(!result[0].message.contains("SELECT clause"));
    }

    #[test]
    fn not_triggered_by_bound_variable() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p HAVING (COUNT(?s) > 1)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_select_alias() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?p (COUNT(?s) AS ?c) WHERE { ?s ?p ?o } GROUP BY ?p HAVING (?c > 1)",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn triggered_despite_trailing_values() {
        // NOTE: The trailing VALUES clause is joined after HAVING, its variables are not visible.
        let text = "SELECT ?p WHERE { ?s ?p ?o } GROUP BY ?p HAVING (?v > 1) VALUES ?v { 1 }";
        let result = run_scoped(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 0));
    }

    #[test]
    fn not_triggered_by_group_by_alias() {
        let result = run_scoped(
            diagnostics,
            "SELECT ?k WHERE { ?s ?p ?o } GROUP BY (STR(?o) AS ?k) HAVING (?k != \"\")",
        );
        assert!(result.is_empty());
    }
}
