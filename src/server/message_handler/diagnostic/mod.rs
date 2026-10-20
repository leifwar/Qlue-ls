mod auto_fix_prefixes;
pub mod deprecated_nearest_neighbors;
pub mod disjoint_minus;
pub mod duplicate_prefix_declaration;
pub mod groupby_star_selection;
pub mod invalid_projection_variable;
pub mod same_subject;
pub mod syntax_error;
pub mod unbound_bind_variable;
pub mod unbound_filter_variable;
pub mod unbound_group_by_expression_variable;
pub mod unbound_group_by_variable;
pub mod unbound_having_variable;
pub mod unbound_order_by_expression_variable;
pub mod unbound_order_by_variable;
pub mod unbound_select_expression_variable;
pub mod uncompacted_uri;
pub mod undeclared_prefix;
pub mod ungrouped_order_by_variable;
pub mod ungrouped_select_variable;
pub mod unused_prefix_declaration;

mod query_scope;

// TODO: Diagnostics for the QLever spatial-join warnings. Their files contain test
// cases only, the bodies are `todo!()` and would panic on every diagnostic request.
// They are deliberately excluded from the build: implement them before enabling the
// modules and registering them in `handle_diagnostic_request`.
// pub mod bounding_box_nearest_neighbors;
// pub mod invalid_spatial_join_payload;

use crate::server::{
    Server,
    lsp::{
        DiagnosticRequest, DiagnosticResponse,
        diagnostic::{CodeDescription, Diagnostic, DiagnosticCode, DiagnosticSeverity},
        errors::LSPError,
        textdocument::{Range, TextDocumentItem},
    },
};
use auto_fix_prefixes::{auto_fix_prefixes, client_support_workspace_edits};
use futures::lock::Mutex;
use ll_sparql_parser::{
    SyntaxNode,
    ast::{AstNode, GraphPatternNotTriples, TriplesBlock, Unit, Var},
    syntax_kind::SyntaxKind,
};
use query_scope::QueryScope;
use std::rc::Rc;

#[tracing::instrument(skip_all, fields(id = %request.get_id(), uri = %request.params.text_document.uri))]
pub(super) async fn handle_diagnostic_request(
    server_rc: Rc<Mutex<Server>>,
    request: DiagnosticRequest,
) -> Result<(), LSPError> {
    let mut server = server_rc.lock().await;
    let document = server
        .state
        .get_document(&request.params.text_document.uri)?;
    let tree = server
        .state
        .get_cached_parse_tree(&request.params.text_document.uri)?
        .tree;

    let ast = Unit::cast(tree).expect("tree should be of kind QueryUnit or UpdateUnit");
    let mut diagnostic_accu = Vec::new();

    // NOTE: The query scopes are shared by all providers that check variable visibility.
    let scopes = QueryScope::all(&ast);

    macro_rules! add_diagnostic {
        ($diagnostic_provider:path) => {
            if let Some(diagnostics) = $diagnostic_provider(document, &ast, &server) {
                diagnostic_accu.extend(diagnostics);
            }
        };
    }
    macro_rules! add_scope_diagnostic {
        ($diagnostic_provider:path) => {
            if let Some(diagnostics) = $diagnostic_provider(document, &scopes) {
                diagnostic_accu.extend(diagnostics);
            }
        };
    }
    add_diagnostic!(unused_prefix_declaration::diagnostics);
    add_diagnostic!(undeclared_prefix::diagnostics);
    add_diagnostic!(uncompacted_uri::diagnostics);
    add_diagnostic!(ungrouped_select_variable::diagnostics);
    add_diagnostic!(invalid_projection_variable::diagnostics);
    add_diagnostic!(same_subject::diagnostics);
    add_diagnostic!(syntax_error::diagnostics);
    add_diagnostic!(duplicate_prefix_declaration::diagnostics);
    add_diagnostic!(groupby_star_selection::diagnostics);
    add_diagnostic!(deprecated_nearest_neighbors::diagnostics);
    add_diagnostic!(disjoint_minus::diagnostics);
    add_diagnostic!(unbound_bind_variable::diagnostics);
    add_diagnostic!(unbound_filter_variable::diagnostics);
    add_scope_diagnostic!(unbound_group_by_variable::diagnostics);
    add_scope_diagnostic!(unbound_group_by_expression_variable::diagnostics);
    add_scope_diagnostic!(unbound_having_variable::diagnostics);
    add_scope_diagnostic!(unbound_order_by_variable::diagnostics);
    add_scope_diagnostic!(unbound_order_by_expression_variable::diagnostics);
    add_scope_diagnostic!(ungrouped_order_by_variable::diagnostics);
    add_scope_diagnostic!(unbound_select_expression_variable::diagnostics);

    if client_support_workspace_edits(&server) {
        auto_fix_prefixes(&mut server, &request, &diagnostic_accu);
    }
    add_code_descriptions(&server, &mut diagnostic_accu);

    server.send_message(DiagnosticResponse::new(request.get_id(), diagnostic_accu))
}

/// Documentation of the diagnostics, every diagnostic has a section anchored by its code.
const DIAGNOSTICS_DOCS_URL: &str = "https://docs.qlue-ls.com/diagnostics/";

/// Link every diagnostic with a code to its section in the documentation,
/// if the client supports `codeDescription`.
pub(super) fn add_code_descriptions(server: &Server, diagnostics: &mut [Diagnostic]) {
    let client_supports_code_description = server
        .client_capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.text_document.as_ref())
        .and_then(|text_document| text_document.publish_diagnostics.as_ref())
        .and_then(|publish_diagnostics| publish_diagnostics.code_description_support)
        .unwrap_or(false);
    if !client_supports_code_description {
        return;
    }
    for diagnostic in diagnostics {
        if let Some(DiagnosticCode::String(code)) = &diagnostic.code {
            diagnostic.code_description = Some(CodeDescription {
                href: format!("{DIAGNOSTICS_DOCS_URL}#{code}"),
            });
        }
    }
}

/// A warning at `node`, mirroring a QLever query warning.
fn warning(
    document: &TextDocumentItem,
    node: &SyntaxNode,
    code: &DiagnosticCode,
    message: String,
) -> Diagnostic {
    Diagnostic {
        range: Range::from_byte_offset_range(node.text_range(), &document.text)
            .expect("Every node should be within the document"),
        severity: DiagnosticSeverity::Warning,
        code: Some(code.clone()),
        code_description: None,
        source: Some("qlue-ls".to_string()),
        message,
        data: None,
    }
}

/// The variables bound by an element of a group graph pattern.
fn bound_variables(element: SyntaxNode) -> Vec<Var> {
    match element.kind() {
        SyntaxKind::TriplesBlock => TriplesBlock::cast(element).map(|tb| tb.visible_variables()),
        SyntaxKind::GraphPatternNotTriples => match GraphPatternNotTriples::cast(element) {
            // NOTE: MINUS and FILTER never bind variables.
            Some(
                GraphPatternNotTriples::MinusGraphPattern(_) | GraphPatternNotTriples::Filter(_),
            ) => None,
            pattern => pattern.map(|pattern| pattern.visible_variables()),
        },
        _ => None,
    }
    .unwrap_or_default()
}

#[cfg(test)]
pub(super) mod test_utils {
    use super::query_scope::QueryScope;
    use crate::server::{
        Server,
        lsp::{
            diagnostic::Diagnostic,
            textdocument::{Range, TextDocumentItem},
        },
    };
    use ll_sparql_parser::{
        ast::{AstNode, Unit},
        parse,
    };
    use text_size::{TextRange, TextSize};

    pub(super) type DiagnosticProvider =
        fn(&TextDocumentItem, &Unit, &Server) -> Option<Vec<Diagnostic>>;

    pub(super) type ScopeDiagnosticProvider =
        fn(&TextDocumentItem, &[QueryScope]) -> Option<Vec<Diagnostic>>;

    /// Run a single diagnostic provider on `text` with a freshly created server.
    ///
    /// NOTE: Providers signal "nothing to report" either with `None` or an empty
    /// `Vec`; both are mapped to an empty `Vec`.
    pub(super) fn run(provider: DiagnosticProvider, text: &str) -> Vec<Diagnostic> {
        run_with_server(provider, &Server::new(|_message| {}), text)
    }

    pub(super) fn run_with_server(
        provider: DiagnosticProvider,
        server: &Server,
        text: &str,
    ) -> Vec<Diagnostic> {
        let document = TextDocumentItem::new("uri", text);
        let (tree, _) = parse(text);
        let ast = Unit::cast(tree).expect("tree should be of kind QueryUnit or UpdateUnit");
        provider(&document, &ast, server).unwrap_or_default()
    }

    /// Run a single provider that works on the query scopes of `text`.
    pub(super) fn run_scoped(provider: ScopeDiagnosticProvider, text: &str) -> Vec<Diagnostic> {
        let document = TextDocumentItem::new("uri", text);
        let (tree, _) = parse(text);
        let ast = Unit::cast(tree).expect("tree should be of kind QueryUnit or UpdateUnit");
        provider(&document, &QueryScope::all(&ast)).unwrap_or_default()
    }

    /// Range of the `occurrence`-th (0-based) match of `needle` in `text`.
    pub(super) fn range_of(text: &str, needle: &str, occurrence: usize) -> Range {
        let (start, _) = text
            .match_indices(needle)
            .nth(occurrence)
            .expect("needle should occur often enough in text");
        Range::from_byte_offset_range(
            TextRange::at(
                TextSize::from(start as u32),
                TextSize::from(needle.len() as u32),
            ),
            text,
        )
        .expect("needle range should be in text")
    }
}

#[cfg(test)]
mod test {
    use super::{
        add_code_descriptions,
        test_utils::{range_of, run, run_scoped, run_with_server},
        unbound_select_expression_variable, undeclared_prefix, ungrouped_select_variable,
    };
    use crate::server::{Server, lsp::diagnostic::CodeDescription};
    use pretty_assertions::assert_eq;

    fn server_with_code_description_support(support: bool) -> Server {
        let mut server = Server::new(|_message| {});
        server.client_capabilities = Some(
            serde_json::from_value(serde_json::json!({
                "textDocument": {
                    "publishDiagnostics": { "codeDescriptionSupport": support }
                }
            }))
            .expect("client capabilities should deserialize"),
        );
        server
    }

    #[test]
    fn code_description_links_to_docs() {
        let server = server_with_code_description_support(true);
        let mut diagnostics = run_with_server(
            undeclared_prefix::diagnostics,
            &server,
            "SELECT * WHERE { ?s ex:p ?o }",
        );
        add_code_descriptions(&server, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code_description,
            Some(CodeDescription {
                href: "https://docs.qlue-ls.com/diagnostics/#undeclared-prefix".to_string()
            })
        );
    }

    #[test]
    fn code_description_omitted_without_client_support() {
        for server in [
            Server::new(|_message| {}),
            server_with_code_description_support(false),
        ] {
            let mut diagnostics = run_with_server(
                undeclared_prefix::diagnostics,
                &server,
                "SELECT * WHERE { ?s ex:p ?o }",
            );
            add_code_descriptions(&server, &mut diagnostics);
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].code_description, None);
        }
    }

    #[test]
    fn ungrouped_select_expression_variable_is_reported_once() {
        // NOTE: The variable is unbound and ungrouped, only the error is reported.
        let text = "SELECT (STR(?x) AS ?y) WHERE { ?s ?p ?o } GROUP BY ?s";
        let diagnostics: Vec<_> = run(ungrouped_select_variable::diagnostics, text)
            .into_iter()
            .chain(run_scoped(
                unbound_select_expression_variable::diagnostics,
                text,
            ))
            .collect();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range, range_of(text, "?x", 0));
        assert_eq!(
            diagnostics[0].code,
            Some((*ungrouped_select_variable::CODE).clone())
        );
    }
}
