//! Flags variables used in a `FILTER` that are not bound in the enclosing
//! group graph pattern.
//!
//! A filter applies to the whole group it occurs in, independent of its
//! position, so every element of the group counts: triples, nested groups,
//! `OPTIONAL`, `SERVICE`, `BIND`, `VALUES` and (selected by) subqueries.
//! Variables inside `MINUS` or another `FILTER` do not count as bound.
//! The group does not see the variables of the groups around it. For an
//! unbound variable the filter expression usually errors, which removes every
//! solution of the group.
//!
//! There are a few exceptions where a filter sees variables from outside its group:
//! - A filter directly inside an `OPTIONAL` becomes the condition of the left
//!   join, it also sees the variables bound before the `OPTIONAL`.
//! - A filter inside `EXISTS`/`NOT EXISTS` is evaluated with the variables of
//!   the outer solution, these filters are not checked at all.
//! - In `GRAPH ?g { ... }`, the graph variable ?g counts as bound.
//!
//! WARNING: By the SPARQL algebra a filter inside `GRAPH ?g { ... }` does not
//! see ?g. Engines are not consistent about this, so it is not flagged.

use super::{bound_variables, query_scope::used_variables, warning};
use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode},
        textdocument::TextDocumentItem,
    },
};
use ll_sparql_parser::{
    SyntaxNode,
    ast::{AstNode, Filter, Unit, Var},
    syntax_kind::SyntaxKind,
};
use std::{collections::HashSet, iter::successors, sync::LazyLock};

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("unbound-filter-var".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = ast
        .syntax()
        .descendants()
        .filter_map(Filter::cast)
        .filter(|filter| !inside_exists(filter.syntax()))
        .flat_map(|filter| {
            let visible_variables = visible_variables(&filter);
            used_variables(filter.syntax())
                .into_iter()
                .filter(move |variable| !visible_variables.contains(&variable.var_name()))
        })
        .map(|variable| {
            let message = format!(
                "The variable {} was used in a FILTER \
                 but is not bound in the enclosing group graph pattern",
                variable.syntax().text()
            );
            warning(document, variable.syntax(), &CODE, message)
        })
        .collect();

    (!diagnostics.is_empty()).then_some(diagnostics)
}

/// The names of the variables `filter` can see.
fn visible_variables(filter: &Filter) -> HashSet<String> {
    // INFO: The FILTER is wrapped in a `GraphPatternNotTriples`, whose parent is the
    // `GroupGraphPatternSub` of the enclosing group.
    let Some(group) = filter
        .syntax()
        .parent()
        .and_then(|pattern| pattern.parent())
        .filter(|group| group.kind() == SyntaxKind::GroupGraphPatternSub)
    else {
        return HashSet::new();
    };
    let group_variables = group.children().flat_map(bound_variables);
    // INFO: GraphPatternNotTriples > (OptionalGraphPattern | GraphGraphPattern) > GroupGraphPattern > GroupGraphPatternSub
    let outer_variables: Vec<Var> = match group.parent().and_then(|ggp| ggp.parent()) {
        Some(optional) if optional.kind() == SyntaxKind::OptionalGraphPattern => successors(
            optional.parent().and_then(|pattern| pattern.prev_sibling()),
            |node| node.prev_sibling(),
        )
        .flat_map(bound_variables)
        .collect(),
        Some(graph) if graph.kind() == SyntaxKind::GraphGraphPattern => graph
            .children()
            .find(|child| child.kind() == SyntaxKind::VarOrIri)
            .and_then(|var_or_iri| var_or_iri.first_child())
            .and_then(Var::cast)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    group_variables
        .chain(outer_variables)
        .map(|var| var.var_name())
        .collect()
}

fn inside_exists(node: &SyntaxNode) -> bool {
    node.ancestors().any(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::ExistsFunc | SyntaxKind::NotExistsFunc
        )
    })
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
    fn triggered_by_unbound_variable() {
        let text = "SELECT * WHERE { ?item <price> ?price FILTER (?pirce > 100) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(diagnostic.range, range_of(text, "?pirce", 0));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(diagnostic.message.contains("?pirce"));
    }

    #[test]
    fn triggered_for_every_occurrence() {
        let text = "SELECT * WHERE { ?s ?p ?o FILTER (?x > 1 && ?x < 10) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
        assert_eq!(result[1].range, range_of(text, "?x", 1));
    }

    #[test]
    fn triggered_by_builtin_call_filter() {
        let text = "SELECT * WHERE { ?s ?p ?o FILTER BOUND(?x) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }

    #[test]
    fn triggered_in_nested_group() {
        // NOTE: The nested group does not see `?o` of the outer group.
        let text = "SELECT * WHERE { ?s ?p ?o { ?s ?q ?v FILTER (?o > 1) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_in_union_branch() {
        let text = "SELECT * WHERE { { ?s <p> ?o } UNION { ?s <q> ?v FILTER (?o > 1) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_by_variable_bound_in_minus() {
        let text = "SELECT * WHERE { ?s ?p ?o MINUS { ?s ?q ?v } FILTER (?v > 1) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 1));
    }

    #[test]
    fn triggered_inside_minus() {
        // NOTE: Unlike OPTIONAL, the group of a MINUS does not see the outer variables.
        let text = "SELECT * WHERE { ?s ?p ?o MINUS { ?s ?q ?v FILTER (?o > 1) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_by_variable_not_selected_by_subquery() {
        let text = "SELECT * WHERE { { SELECT ?s WHERE { ?s ?p ?o } } FILTER (?o > 1) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_by_trailing_values() {
        // NOTE: The trailing VALUES clause is joined after the WHERE clause.
        let text = "SELECT * WHERE { ?s ?p ?o FILTER (?v > 1) } VALUES ?v { 1 }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?v", 0));
    }

    #[test]
    fn triggered_in_optional_by_variable_bound_after_optional() {
        let text = "SELECT * WHERE { ?s ?p ?o OPTIONAL { ?s ?q ?v FILTER (?w > ?v) } ?s ?r ?w }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?w", 0));
    }

    #[test]
    fn triggered_in_group_nested_in_optional() {
        // NOTE: Only the filters of the OPTIONAL group itself become the left join condition.
        let text = "SELECT * WHERE { ?s ?p ?o OPTIONAL { { ?s ?q ?v FILTER (?o > ?v) } } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?o", 1));
    }

    #[test]
    fn triggered_in_update_where_clause() {
        let text = "DELETE { ?s ?p ?o } WHERE { ?s ?p ?o FILTER (?x > 1) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "?x", 0));
    }

    #[test]
    fn not_triggered_by_preceding_triple() {
        let result = run(diagnostics, "SELECT * WHERE { ?s ?p ?o FILTER (?o > 1) }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_bound_after_filter() {
        // NOTE: A filter applies to the whole group, independent of its position.
        let result = run(diagnostics, "SELECT * WHERE { FILTER (?o > 1) ?s ?p ?o }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_bind() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o BIND (STR(?o) AS ?x) FILTER (?x != \"\") }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_values() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { VALUES ?o { 1 2 } FILTER (?o > 1) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_optional() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o OPTIONAL { ?o ?q ?v } FILTER (!BOUND(?v)) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_nested_group() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { { ?s ?p ?o } FILTER (?o > 1) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_union() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { { ?s <p> ?o } UNION { ?s <q> ?o } FILTER (?o > 1) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_service() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { SERVICE <endpoint> { ?s ?p ?o } FILTER (?o > 1) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_variable_selected_by_subquery() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { { SELECT ?s WHERE { ?s ?p ?o } } FILTER (isIRI(?s)) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_in_optional_by_variable_bound_before_optional() {
        // NOTE: The filter of an OPTIONAL is the condition of the left join.
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o OPTIONAL { ?s ?q ?v FILTER (?v > ?o) } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_graph_variable() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { GRAPH ?g { ?s ?p ?o FILTER (?g != <default>) } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_inside_exists() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o FILTER EXISTS { ?s ?q ?v FILTER (?v > ?o) } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_exists_pattern() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { ?s ?p ?o FILTER NOT EXISTS { ?s <q> ?v } }",
        );
        assert!(result.is_empty());
    }
}
