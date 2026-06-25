//! Document symbol collection and identifier search for the LSP server.

use ropey::Rope;
pub(crate) use tower_lsp::lsp_types::SymbolKind as SymbolKind2;
use tower_lsp::lsp_types::{Location, SymbolInformation, Url};

use assura_parser::ast::{
    BindDecl, BlockKind, Clause, CodecRegistryDecl, ContractDecl, DeclVisitor, EnumDef, ExternDecl,
    FnDef, ProphecyDecl, ServiceDecl, ServiceItem, SourceFile, TypeDef,
};

use crate::util::{byte_span_to_range, is_ident_char};

/// Find all word-boundary occurrences of an identifier in the source text.
pub(crate) fn find_identifier_occurrences(
    source: &str,
    name: &str,
    rope: &Rope,
    uri: &Url,
) -> Vec<Location> {
    let mut locations = Vec::new();
    let name_len = name.len();
    let bytes = source.as_bytes();

    let mut start = 0;
    while let Some(pos) = source[start..].find(name) {
        let abs_pos = start + pos;
        let end_pos = abs_pos + name_len;

        // Check word boundaries: must not be preceded or followed by ident chars
        let preceded_by_ident = abs_pos > 0 && is_ident_char(bytes[abs_pos - 1]);
        let followed_by_ident = end_pos < bytes.len() && is_ident_char(bytes[end_pos]);

        if !preceded_by_ident && !followed_by_ident {
            let range = byte_span_to_range(rope, &(abs_pos..end_pos));
            locations.push(Location::new(uri.clone(), range));
        }

        start = abs_pos + 1;
    }

    locations
}

/// Check if a string is a valid Assura identifier.
pub(crate) fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------
// Document symbol collection
// ---------------------------------------------------------------------------

/// Collect top-level declarations as flat `SymbolInformation` entries.
///
/// Uses [`DeclVisitor`] for variant dispatch; walks decls manually so each
/// symbol keeps its source span (walk_decls does not pass spans).
#[allow(deprecated)] // SymbolInformation::deprecated is deprecated but required by the type
pub(crate) fn collect_document_symbols(
    ast: &SourceFile,
    rope: &Rope,
    doc_uri: &Url,
) -> Vec<SymbolInformation> {
    struct SymbolCollector<'a> {
        result: Vec<SymbolInformation>,
        doc_uri: &'a Url,
        range: tower_lsp::lsp_types::Range,
    }

    impl SymbolCollector<'_> {
        fn push(&mut self, name: String, kind: SymbolKind2, container: Option<String>) {
            self.result.push(SymbolInformation {
                name,
                kind,
                tags: None,
                deprecated: None,
                location: Location::new(self.doc_uri.clone(), self.range),
                container_name: container,
            });
        }
    }

    impl DeclVisitor for SymbolCollector<'_> {
        fn visit_contract(&mut self, c: &ContractDecl) {
            self.push(c.name.clone(), SymbolKind2::CLASS, None);
        }
        fn visit_service(&mut self, s: &ServiceDecl) {
            self.push(s.name.clone(), SymbolKind2::MODULE, None);
            for item in &s.items {
                let child_name = match item {
                    ServiceItem::TypeDef(t) => Some((t.name.clone(), SymbolKind2::CLASS)),
                    ServiceItem::EnumDef(e) => Some((e.name.clone(), SymbolKind2::ENUM)),
                    ServiceItem::Operation { name, .. } => {
                        Some((name.clone(), SymbolKind2::METHOD))
                    }
                    ServiceItem::Query { name, .. } => Some((name.clone(), SymbolKind2::METHOD)),
                    _ => None,
                };
                if let Some((name, kind)) = child_name {
                    self.push(name, kind, Some(s.name.clone()));
                }
            }
        }
        fn visit_type_def(&mut self, t: &TypeDef) {
            self.push(t.name.clone(), SymbolKind2::STRUCT, None);
        }
        fn visit_enum_def(&mut self, e: &EnumDef) {
            self.push(e.name.clone(), SymbolKind2::ENUM, None);
        }
        fn visit_fn_def(&mut self, f: &FnDef) {
            self.push(f.name.clone(), SymbolKind2::FUNCTION, None);
        }
        fn visit_extern(&mut self, ex: &ExternDecl) {
            self.push(ex.name.clone(), SymbolKind2::FUNCTION, None);
        }
        fn visit_bind(&mut self, b: &BindDecl) {
            self.push(b.name.clone(), SymbolKind2::FUNCTION, None);
        }
        fn visit_prophecy(&mut self, p: &ProphecyDecl) {
            self.push(p.name.clone(), SymbolKind2::VARIABLE, None);
        }
        fn visit_codec_registry(&mut self, cr: &CodecRegistryDecl) {
            self.push(cr.name.clone(), SymbolKind2::MODULE, None);
        }
        fn visit_block(
            &mut self,
            _kind: &BlockKind,
            name: &str,
            _value: &Option<Vec<String>>,
            _body: &[Clause],
        ) {
            if !name.is_empty() {
                self.push(name.to_string(), SymbolKind2::NAMESPACE, None);
            }
        }
    }

    let mut result = Vec::new();
    for decl in &ast.decls {
        let range = byte_span_to_range(rope, &decl.span);
        let mut visitor = SymbolCollector {
            result: Vec::new(),
            doc_uri,
            range,
        };
        visitor.visit_decl(&decl.node);
        result.append(&mut visitor.result);
    }

    result
}
