use ll_sparql_parser::{
    SyntaxToken,
    ast::{AstNode, Iri, ServiceGraphPattern, Unit},
};

use crate::server::{
    Server, configuration::BackendConfiguration, lsp::SparqlEngine, uri_converter::Converter,
};

/// Resolve which Backend to use at given token.
/// Currently only works for Query operations.
pub(super) fn resolve_backend_at_token(
    server: &Server,
    query_unit: &Unit,
    token: &SyntaxToken,
) -> Option<BackendConfiguration> {
    token
        .parent_ancestors()
        .find_map(ServiceGraphPattern::cast)
        .and_then(|service| {
            service
                .iri()
                .and_then(|iri| iri.raw_iri())
                .or(service.iri().and_then(|iri| {
                    iri.prefixed_name().and_then(|prefixed_name| {
                        query_unit.prologue().and_then(|prologue| {
                            prologue
                                .prefix_declarations()
                                .iter()
                                .find_map(|prefix_declaration| {
                                    prefix_declaration
                                        .prefix()
                                        .is_some_and(|prefix| prefix == prefixed_name.prefix())
                                        .then_some(prefix_declaration.raw_uri_prefix())
                                        .flatten()
                                })
                        })
                    })
                }))
        })
        .and_then(|iri_string| server.state.get_backend_name_by_url(&iri_string))
        .and_then(|backend_name| server.state.get_backend(&backend_name).cloned())
        .or(server.state.get_default_backend().cloned())
}

/// Whether the backend at given token might be QLever.
/// This is the case unless the backend is known to run a different engine,
/// i.e. no backend or a backend without configured engine counts as QLever.
pub(super) fn may_be_qlever_at_token(server: &Server, ast: &Unit, token: &SyntaxToken) -> bool {
    resolve_backend_at_token(server, ast, token)
        .and_then(|backend| backend.engine)
        .is_none_or(|engine| engine == SparqlEngine::QLever)
}

/// Creates the prefix declaration for any IRI.
/// If the iri is raw (no prefix used) None is returned.
/// If the iri is a prefixed name the uri prefix is resolved in this order:
/// 1. Is there a prefix declaration at the top of the document declaring this prefix.
///    The first prefix found is used.
/// 2. Is this prefix stored in the iri converter of the given backend.
pub(super) fn get_iri_prefix_declaration(
    iri: &Iri,
    ast: &Unit,
    converter: &Converter,
) -> Option<(String, String)> {
    get_prologue_prefix_declaration(iri, ast).or_else(|| {
        let used_prefix = iri.prefixed_name()?.prefix();
        converter
            .find_by_prefix(&used_prefix)
            .cloned()
            .map(|record| (used_prefix, record.uri_prefix))
    })
}

/// Finds the prefix declaration at the top of the document for any IRI.
/// If the iri is raw (no prefix used) or its prefix is not declared, None is returned.
/// If the prefix is declared multiple times, the first declaration is used.
pub(super) fn get_prologue_prefix_declaration(iri: &Iri, ast: &Unit) -> Option<(String, String)> {
    let used_prefix = iri.prefixed_name()?.prefix();
    ast.prologue().and_then(|prologue| {
        prologue
            .prefix_declarations()
            .iter()
            .find_map(|prefix_declaration| {
                let prefix = prefix_declaration.prefix()?;
                let uri_prefix = prefix_declaration.raw_uri_prefix()?;
                (used_prefix == prefix).then_some((prefix, uri_prefix))
            })
    })
}
