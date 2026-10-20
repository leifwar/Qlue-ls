//! Flags rows of a `VALUES` block whose number of values does not match the
//! number of variables in its header.
//!
//! QLever rejects such a query with the error (`SparqlQleverVisitor.cpp`):
//! "The number of values in every data block must match the number of
//! variables in a values clause."
//!
//! Both the inline `VALUES` of a group graph pattern and the trailing `VALUES`
//! clause are checked. A missing value is written as `UNDEF`.
//!
//! NOTE: The rows of `InlineDataFull` are not wrapped in their own nodes, they
//! are recovered from the `(`, `)` and `()` tokens. Rows that are not closed
//! (while typing) are skipped, the parser reports them as syntax errors.

use crate::server::{
    Server,
    lsp::{
        diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity},
        textdocument::{Range, TextDocumentItem},
    },
};
use ll_sparql_parser::{
    SyntaxElement, SyntaxNode,
    ast::{AstNode, Unit},
    syntax_kind::SyntaxKind,
};
use std::sync::LazyLock;
use text_size::TextRange;

pub static CODE: LazyLock<DiagnosticCode> =
    LazyLock::new(|| DiagnosticCode::String("invalid-values-row".to_string()));

pub(super) fn diagnostics(
    document: &TextDocumentItem,
    ast: &Unit,
    _server: &Server,
) -> Option<Vec<Diagnostic>> {
    let diagnostics: Vec<Diagnostic> = ast
        .syntax()
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::InlineDataFull)
        .filter_map(|data_block| Some((header_width(&data_block)?, rows(&data_block))))
        .flat_map(|(width, rows)| {
            rows.into_iter()
                .filter(move |row| row.width != width)
                .map(move |row| (row, width))
        })
        .map(|(row, width)| Diagnostic {
            range: Range::from_byte_offset_range(row.range, &document.text)
                .expect("Every row should be within the document"),
            severity: DiagnosticSeverity::Error,
            code: Some((*CODE).clone()),
            code_description: None,
            source: Some("qlue-ls".to_string()),
            message: format!(
                "This row has {}, but the VALUES clause declares {}. \
                 Use UNDEF for a missing value.",
                plural(row.width, "value"),
                plural(width, "variable")
            ),
            data: None,
        })
        .collect();

    (!diagnostics.is_empty()).then_some(diagnostics)
}

/// A row of an `InlineDataFull` block.
struct Row {
    range: TextRange,
    width: usize,
}

/// The number of variables in the header `( ?x ?y )` (or `()`), if it is complete.
fn header_width(data_block: &SyntaxNode) -> Option<usize> {
    let mut width = 0;
    for element in data_block.children_with_tokens() {
        match element.kind() {
            SyntaxKind::NIL | SyntaxKind::RParen => return Some(width),
            SyntaxKind::Var => width += 1,
            SyntaxKind::LCurly => return None,
            _ => {}
        }
    }
    None
}

/// The closed rows after the `{` of the block.
fn rows(data_block: &SyntaxNode) -> Vec<Row> {
    let mut rows = Vec::new();
    // INFO: The start and the number of values of the row that is currently open.
    let mut open_row: Option<(SyntaxElement, usize)> = None;
    for element in data_block
        .children_with_tokens()
        .skip_while(|element| element.kind() != SyntaxKind::LCurly)
    {
        match element.kind() {
            SyntaxKind::NIL => rows.push(Row {
                range: element.text_range(),
                width: 0,
            }),
            SyntaxKind::LParen => open_row = Some((element, 0)),
            SyntaxKind::DataBlockValue => {
                if let Some((_, width)) = open_row.as_mut() {
                    *width += 1;
                }
            }
            SyntaxKind::RParen => {
                if let Some((start, width)) = open_row.take() {
                    rows.push(Row {
                        range: start.text_range().cover(element.text_range()),
                        width,
                    });
                }
            }
            _ => {}
        }
    }
    rows
}

fn plural(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
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
    fn triggered_by_short_and_long_rows() {
        let text = "SELECT ?x ?y WHERE { VALUES (?x ?y) { (1) (2 3 4) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].range, range_of(text, "(1)", 0));
        assert_eq!(result[0].severity, DiagnosticSeverity::Error);
        assert_eq!(result[0].code, Some((*CODE).clone()));
        assert_eq!(
            result[0].message,
            "This row has 1 value, but the VALUES clause declares 2 variables. \
             Use UNDEF for a missing value."
        );
        assert_eq!(result[1].range, range_of(text, "(2 3 4)", 0));
        assert!(result[1].message.contains("3 values"));
    }

    #[test]
    fn triggered_by_empty_row() {
        let text = "SELECT * WHERE { VALUES (?x) { () ( ) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].range, range_of(text, "()", 0));
        assert_eq!(result[1].range, range_of(text, "( )", 0));
        assert!(result[0].message.contains("0 values"));
    }

    #[test]
    fn triggered_by_row_in_empty_header() {
        let text = "SELECT * WHERE { VALUES () { (1) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "(1)", 0));
        assert!(result[0].message.contains("0 variables"));
    }

    #[test]
    fn triggered_in_trailing_values_clause() {
        let text = "SELECT * WHERE { ?s ?p ?o } VALUES (?s ?o) { (<a> <b>) (<c>) }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "(<c>)", 0));
    }

    #[test]
    fn triggered_in_update() {
        let text = "INSERT { ?s <p> ?o } WHERE { VALUES (?s ?o) { (<a>) } }";
        let result = run(diagnostics, text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].range, range_of(text, "(<a>)", 0));
    }

    #[test]
    fn not_triggered_by_matching_rows() {
        let result = run(
            diagnostics,
            "SELECT * WHERE { VALUES (?x ?y) { (1 2) (UNDEF <a>) (\"s\" true) } }",
        );
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_empty_header_and_rows() {
        let result = run(diagnostics, "SELECT * WHERE { VALUES () { () ( ) } }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_single_variable() {
        let result = run(diagnostics, "SELECT * WHERE { VALUES ?x { 1 2 3 } }");
        assert!(result.is_empty());
    }

    #[test]
    fn not_triggered_by_unclosed_row() {
        // NOTE: The parser reports the unclosed row as syntax error.
        let result = run(
            diagnostics,
            "SELECT * WHERE { VALUES (?x ?y) { (1 2) (3 } }",
        );
        assert!(result.is_empty());
    }
}
