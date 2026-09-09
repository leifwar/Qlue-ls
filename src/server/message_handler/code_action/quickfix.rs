use super::Diagnostic;
use crate::server::{
    Server,
    analysis::namespace_is_declared,
    common::{UncompactedUrisDiagnosticData, serde_parse},
    lsp::{
        CodeAction, CodeActionKind, WorkspaceEdit,
        base_types::LSPAny,
        errors::{ErrorCode, LSPError},
        textdocument::{Position, Range, TextEdit},
    },
    message_handler::{code_action::same_subject::contract_triples_from_diagnostic, diagnostic},
};
use ll_sparql_parser::syntax_kind::SyntaxKind;
use std::collections::HashMap;
use text_size::TextRange;

pub(super) fn get_quickfix(
    server: &mut Server,
    document_uri: &String,
    diagnostic: Diagnostic,
) -> Result<Option<CodeAction>, LSPError> {
    match diagnostic.code.as_ref() {
        Some(code) if code == &*diagnostic::undeclared_prefix::CODE => {
            declare_prefix(server, document_uri, diagnostic)
        }
        Some(code) if code == &*diagnostic::uncompacted_uri::CODE => {
            shorten_uri(server, document_uri, diagnostic)
        }
        Some(code) if code == &*diagnostic::unused_prefix_declaration::CODE => {
            remove_prefix_declaration(server, document_uri, diagnostic)
        }
        Some(code) if code == &*diagnostic::same_subject::CODE => {
            contract_triples_from_diagnostic(server, document_uri, diagnostic)
        }
        Some(code) if code == &*diagnostic::duplicate_prefix_declaration::CODE => {
            remove_prefix_declaration(server, document_uri, diagnostic)
        }
        Some(_) => {
            // NOTE:Not all diagnostics have a quickfix
            Ok(None)
        }
        None => Ok(None),
    }
}

pub(crate) fn remove_prefix_declaration(
    server: &mut Server,
    document_uri: &String,
    diagnostic: Diagnostic,
) -> Result<Option<CodeAction>, LSPError> {
    let optimal_range = server
        .state
        .get_cached_parse_tree(document_uri)
        .ok()
        .and_then(|result| {
            let tree = result.tree;
            server
                .state
                .get_document(document_uri)
                .ok()
                .and_then(|document| {
                    diagnostic
                        .range
                        .to_byte_index_range(&document.text)
                        .and_then(|text_range| {
                            let prefix_decl = tree.covering_element(text_range);
                            (prefix_decl.kind() == SyntaxKind::PrefixDecl)
                                .then(|| {
                                    assert!(matches!(prefix_decl.kind(), SyntaxKind::PrefixDecl));
                                    let mut maybe_next = prefix_decl
                                        .as_node()
                                        .and_then(|node| node.last_token())
                                        .and_then(|token| token.next_token());
                                    while let Some(next) = maybe_next.as_ref() {
                                        if next.kind().is_trivia() {
                                            maybe_next = next.next_token();
                                        } else {
                                            break;
                                        }
                                    }
                                    maybe_next.and_then(|next| {
                                        Range::from_byte_offset_range(
                                            TextRange::new(
                                                text_range.start(),
                                                next.text_range().start(),
                                            ),
                                            &document.text,
                                        )
                                    })
                                })
                                .flatten()
                        })
                })
        });
    let range = optimal_range.unwrap_or(diagnostic.range);
    let mut code_action =
        CodeAction::new("remove prefix declaration", Some(CodeActionKind::QuickFix));
    code_action.add_edit(document_uri, TextEdit::new(range, ""));
    Ok(Some(code_action))
}

// NOTE: PREFIX declarations should be inserted:
// - after any comments
// - after the existing prefix declarations
fn prefix_declaration_insert_line(server: &Server, document_uri: &str) -> Result<u32, LSPError> {
    let tree = server.state.get_cached_parse_tree(document_uri)?.tree;
    let document = server.state.get_document(document_uri)?;

    Ok(if let Some(node) = tree.first_child() {
        if let Some(prologue) = node.first_child()
            && prologue.kind() == SyntaxKind::Prologue
        {
            Position::from_byte_index(prologue.text_range().end(), &document.text)
                .unwrap()
                .line
                + 1
        } else {
            Position::from_byte_index(node.text_range().start(), &document.text)
                .unwrap()
                .line
        }
    } else {
        tree.children_with_tokens()
            .take_while(|child| {
                child
                    .as_token()
                    .is_some_and(|token| token.kind().is_trivia())
            })
            .count() as u32
    })
}

fn shorten_uri(
    server: &mut Server,
    document_uri: &String,
    diagnostic: Diagnostic,
) -> Result<Option<CodeAction>, LSPError> {
    match diagnostic.data {
        Some(data) => {
            let UncompactedUrisDiagnosticData(prefix, namespace, curie): UncompactedUrisDiagnosticData =
                serde_parse(data)?;
            let mut code_action = CodeAction::new("Shorten URI", Some(CodeActionKind::QuickFix));
            code_action.add_edit(document_uri, TextEdit::new(diagnostic.range, &curie));
            if !namespace_is_declared(&server.state, document_uri, &prefix)? {
                let insert_line = prefix_declaration_insert_line(server, document_uri)?;
                code_action.add_edit(
                    document_uri,
                    TextEdit::new(
                        Range::new(insert_line, 0, insert_line, 0),
                        &format!("PREFIX {}: <{}>\n", prefix, namespace),
                    ),
                );
            }
            Ok(Some(code_action))
        }
        None => {
            tracing::error!("Data-field is missing in \"uncompacted-uri\" diagnostic");
            Err(LSPError::new(
                ErrorCode::InvalidRequest,
                "Data-field is missing in \"uncompacted-uri\" diagnostic",
            ))
        }
    }
}

pub(crate) fn declare_prefix(
    server: &Server,
    document_uri: &str,
    diagnostic: Diagnostic,
) -> Result<Option<CodeAction>, LSPError> {
    if let Some(LSPAny::String(prefix)) = &diagnostic.data {
        if let Some(Ok(record)) = server
            .state
            .get_default_converter()
            .map(|converter| converter.find_by_prefix(prefix))
        {
            let insert_line = prefix_declaration_insert_line(server, document_uri)?;
            Ok(Some(CodeAction {
                title: format!("Declare prefix \"{}\"", prefix),
                kind: Some(CodeActionKind::QuickFix),
                edit: WorkspaceEdit {
                    changes: Some(HashMap::from([(
                        document_uri.to_string(),
                        vec![TextEdit::new(
                            Range::new(insert_line, 0, insert_line, 0),
                            &format!("PREFIX {}: <{}>\n", prefix, record.uri_prefix),
                        )],
                    )])),
                },
                diagnostics: vec![diagnostic],
            }))
        } else {
            Ok(None)
        }
    } else {
        Err(LSPError::new(
            ErrorCode::InvalidParams,
            "expected prefix in undeclared-prefix data... was disappointed",
        ))
    }
}

#[cfg(test)]
mod test {

    use crate::server::{
        Server,
        lsp::{
            base_types::LSPAny,
            diagnostic::{self, Diagnostic},
            textdocument::{Range, TextDocumentItem, TextEdit},
        },
        message_handler::code_action::quickfix::shorten_uri,
        state::ServerState,
    };
    use indoc::indoc;

    fn setup_state(text: &str) -> ServerState {
        let mut state = ServerState::new();
        let document = TextDocumentItem::new("uri", text);
        state.add_document(document);
        state
    }
    #[test]
    fn shorten_uri_undeclared() {
        let mut server = Server::new(|_message| {});
        let state = setup_state(indoc!(
            "SELECT * {
               ?a <http://schema.org/name> ?b .
             }"
        ));
        server.state = state;
        let diagnostic = Diagnostic {
            range: Range::new(1, 5, 1, 29),
            severity: diagnostic::DiagnosticSeverity::Information,
            message: String::new(),
            source: None,
            code: None,
            data: Some(LSPAny::LSPArray(vec![
                LSPAny::String("schema".to_string()),
                LSPAny::String("http://schema.org/".to_string()),
                LSPAny::String("schema:name".to_string()),
            ])),
        };

        let code_action = shorten_uri(&mut server, &"uri".to_string(), diagnostic)
            .unwrap()
            .unwrap();

        assert_eq!(
            code_action.edit.changes.unwrap().get("uri").unwrap(),
            &vec![
                TextEdit::new(Range::new(1, 5, 1, 29), "schema:name"),
                TextEdit::new(
                    Range::new(0, 0, 0, 0),
                    "PREFIX schema: <http://schema.org/>\n"
                ),
            ]
        );
    }

    #[test]
    fn shorten_uri_declared() {
        let mut server = Server::new(|_message| {});
        let state = setup_state(indoc!(
            "PREFIX schema: <http://schema.org/>
             SELECT * {
               ?a <http://schema.org/name> ?b .
             }"
        ));
        server.state = state;
        let diagnostic = Diagnostic {
            range: Range::new(2, 5, 2, 29),
            severity: diagnostic::DiagnosticSeverity::Information,
            message: String::new(),
            source: None,
            code: None,
            data: Some(LSPAny::LSPArray(vec![
                LSPAny::String("schema".to_string()),
                LSPAny::String("http://schema.org".to_string()),
                LSPAny::String("schema:name".to_string()),
            ])),
        };

        let code_action = shorten_uri(&mut server, &"uri".to_string(), diagnostic)
            .unwrap()
            .unwrap();
        assert_eq!(
            code_action.edit.changes.unwrap().get("uri").unwrap(),
            &vec![TextEdit::new(Range::new(2, 5, 2, 29), "schema:name"),]
        );
    }

    #[test]
    fn shorten_uri_undeclared_after_existing_prefix() {
        let mut server = Server::new(|_message| {});
        let state = setup_state(indoc!(
            "PREFIX schema: <http://schema.org/>
             SELECT * {
               ?a ?b <http://example.org/name> .
             }"
        ));
        server.state = state;
        let diagnostic = Diagnostic {
            range: Range::new(2, 8, 2, 34),
            severity: diagnostic::DiagnosticSeverity::Information,
            message: String::new(),
            source: None,
            code: None,
            data: Some(LSPAny::LSPArray(vec![
                LSPAny::String("ex".to_string()),
                LSPAny::String("http://example.org/".to_string()),
                LSPAny::String("ex:name".to_string()),
            ])),
        };

        let code_action = shorten_uri(&mut server, &"uri".to_string(), diagnostic)
            .unwrap()
            .unwrap();

        assert_eq!(
            code_action.edit.changes.unwrap().get("uri").unwrap(),
            &vec![
                TextEdit::new(Range::new(2, 8, 2, 34), "ex:name"),
                TextEdit::new(Range::new(1, 0, 1, 0), "PREFIX ex: <http://example.org/>\n"),
            ]
        );
    }
}
