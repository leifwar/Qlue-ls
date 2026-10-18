//! Flags uses of QLever's deprecated `<nearest-neighbors:...>` special
//! predicate.
//!
//! Mirrors the QLever warning (`QueryPlanner.cpp`):
//! "The special predicate <nearest-neighbors:...> is deprecated due to
//! confusing semantics. Please upgrade your query to the new syntax 'SERVICE
//! <spatial-search IRI> { ... }'. For more information, please see the QLever
//! Docs (https://docs.qlever.dev/geosparql/)."
//!
//! Only IRIs that start with `nearest-neighbors:` are affected, the
//! `<max-distance-in-meters:...>` predicate is not deprecated. Prefixed names
//! (e.g. `nn:20` with `PREFIX nn: <nearest-neighbors:>`) are expanded first. Like QLever,
//! only the predicate of a triple pattern is checked: anywhere else (or as
//! part of a property path) the IRI has no special meaning.
//!
//! The IRI is only special to QLever, so it is not flagged where the backend
//! (the default one, or the one of the enclosing `SERVICE`) is known to run a
//! different engine.

use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity},
        textdocument::{Range, TextDocumentItem},
    },
    message_handler::misc::{get_prologue_prefix_declaration, may_be_qlever_at_token},
};
use ll_sparql_parser::{
    SyntaxNode,
    ast::{AstNode, Iri, Unit},
    syntax_kind::SyntaxKind,
};
use std::sync::LazyLock;

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("deprecated-nearest-neighbors".to_string()));

const MESSAGE: &str = "The special predicate <nearest-neighbors:...> is deprecated due to confusing semantics.\n\
    Please upgrade your query to the new syntax 'SERVICE <https://qlever.cs.uni-freiburg.de/spatialSearch/> { ... }'.\n\
    For more information, please see the QLever Docs (https://docs.qlever.dev/geosparql/).";

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    server: &Server,
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = ast
        .syntax()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::VerbPath)
        .filter_map(|verb| plain_iri(&verb))
        .filter(|iri| {
            expanded_iri(iri, ast).is_some_and(|iri| iri.starts_with("nearest-neighbors:"))
        })
        .filter(|iri| {
            iri.syntax()
                .first_token()
                .is_some_and(|token| may_be_qlever_at_token(server, ast, &token))
        })
        .map(|iri| Diagnostic {
            range: Range::from_byte_offset_range(iri.syntax().text_range(), &document.text)
                .expect("IRI text range should be in text"),
            severity: DiagnosticSeverity::Warning,
            code: Some((*CODE).clone()),
            code_description: None,
            source: Some("qlue-ls".to_string()),
            message: MESSAGE.to_string(),
            data: None,
        })
        .collect();
    (!diagnostics.is_empty()).then_some(diagnostics)
}

/// The IRI a verb path consists of, if the path is nothing but that IRI
/// (i.e. not `<a>*`, `^<a>` or `<a>/<b>`).
fn plain_iri(verb: &SyntaxNode) -> Option<Iri> {
    let iri = verb.descendants().find_map(Iri::cast)?;
    let iri_range = iri.syntax().text_range();
    verb.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia())
        .all(|token| iri_range.contains_range(token.text_range()))
        .then_some(iri)
}

/// The full IRI, with the prefix of a prefixed name resolved via the prologue.
///
/// NOTE: The backend's prefix map is not consulted: QLever only knows the
/// prefixes declared in the query, undeclared ones are flagged by `undeclared_prefix`.
fn expanded_iri(iri: &Iri, ast: &Unit) -> Option<String> {
    iri.raw_iri().or_else(|| {
        let (_, uri_prefix) = get_prologue_prefix_declaration(iri, ast)?;
        Some(format!("{}{}", uri_prefix, iri.prefixed_name()?.name()))
    })
}

#[cfg(test)]
mod test {
    use super::{CODE, diagnostics};
    use crate::server::{
        Server,
        configuration::BackendConfiguration,
        lsp::{SparqlEngine, diagnostic::DiagnosticSeverity},
        message_handler::diagnostic::test_utils::{range_of, run, run_with_server},
    };
    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn backend(name: &str, url: &str, engine: Option<SparqlEngine>) -> BackendConfiguration {
        BackendConfiguration {
            name: name.to_string(),
            url: url.to_string(),
            health_check_url: None,
            engine,
            request_method: None,
            prefix_map: HashMap::new(),
            default: false,
            queries: HashMap::new(),
            additional_data: None,
        }
    }

    /// A server whose default backend runs `default_engine`, plus a QLever
    /// backend at `https://qlever.example.org`.
    fn setup_server(default_engine: Option<SparqlEngine>) -> Server {
        let mut server = Server::new(|_message| {});
        server.state.add_backend(backend(
            "default",
            "https://default.example.org",
            default_engine,
        ));
        server.state.add_backend(backend(
            "qlever",
            "https://qlever.example.org",
            Some(SparqlEngine::QLever),
        ));
        server.state.set_default_backend("default".to_string());
        server
    }

    const QUERY: &str =
        "SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y <nearest-neighbors:20> ?b }";

    #[test]
    fn triggered_for_qlever_backend() {
        let result = run_with_server(
            diagnostics,
            &setup_server(Some(SparqlEngine::QLever)),
            QUERY,
        );
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn triggered_for_backend_without_engine() {
        let result = run_with_server(diagnostics, &setup_server(None), QUERY);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn triggered_in_service_of_qlever_backend() {
        let text = indoc!(
            "SELECT ?x ?y WHERE {
               SERVICE <https://qlever.example.org> {
                 ?x <p> ?y . ?a <p> ?b . ?y <nearest-neighbors:20> ?b
               }
             }"
        );
        let result = run_with_server(
            diagnostics,
            &setup_server(Some(SparqlEngine::GraphDB)),
            text,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "<nearest-neighbors:20>", 0));
    }

    #[test]
    fn not_triggered_for_other_engine() {
        let result = run_with_server(
            diagnostics,
            &setup_server(Some(SparqlEngine::GraphDB)),
            QUERY,
        );
        assert!(result.is_empty());
    }

    #[test]
    fn triggered_by_nearest_neighbors_with_distance() {
        let text = "SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y <nearest-neighbors:2:500> ?b }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        let diagnostic = &result[0];
        assert_eq!(
            diagnostic.range,
            range_of(text, "<nearest-neighbors:2:500>", 0)
        );
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.code, Some((*CODE).clone()));
        assert!(diagnostic.message.contains("SERVICE"));
    }

    #[test]
    fn triggered_by_nearest_neighbors_without_distance() {
        let text = "SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y <nearest-neighbors:20> ?b }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "<nearest-neighbors:20>", 0));
    }

    #[test]
    fn triggered_in_predicate_list() {
        let text =
            "SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y <p> ?x ; <nearest-neighbors:20> ?b }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "<nearest-neighbors:20>", 0));
    }

    #[test]
    fn triggered_by_prefixed_name() {
        let text = indoc!(
            "PREFIX nn: <nearest-neighbors:>
             SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y nn:20 ?b }"
        );
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "nn:20", 0));
    }

    #[test]
    fn not_triggered_by_undeclared_prefix() {
        let result = run(
            diagnostics,
            "SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y nn:20 ?b }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_prefix_of_other_namespace() {
        let result = run(
            diagnostics,
            indoc!(
                "PREFIX nn: <http://example.org/nearest-neighbors:>
                 SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y nn:20 ?b }"
            ),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_in_object_position() {
        let result = run(
            diagnostics,
            "SELECT ?x WHERE { ?x <p> <nearest-neighbors:20> }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_in_filter() {
        let result = run(
            diagnostics,
            "SELECT ?x WHERE { ?x ?p ?o FILTER(?p = <nearest-neighbors:20>) }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_in_property_path() {
        let result = run(
            diagnostics,
            "SELECT ?x WHERE { ?x <nearest-neighbors:20>/<p> ?y . ?y ^<nearest-neighbors:20> ?z }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_max_distance() {
        let result = run(
            diagnostics,
            "SELECT ?x ?y WHERE { ?x <p> ?y . ?a <p> ?b . ?y <max-distance-in-meters:500> ?b }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_similar_iri() {
        let result = run(
            diagnostics,
            "SELECT ?x WHERE { ?x <http://example.org/nearest-neighbors> ?y }",
        );
        assert!(result.is_empty());
    }
}
