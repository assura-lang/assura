//! Type reference resolution (T012): checking that type names in fields,
//! parameters, and return types resolve to known types.

use assura_parser::ast::{
    BindDecl, DeclVisitor, EnumDef, ExternDecl, FieldDef, FnDef, Param, ServiceDecl, ServiceItem,
    SourceFile, Span, TypeBody, TypeDef,
};

use crate::errors::ResolutionError;
use crate::imports::{ImportStatus, ResolvedImport};
use crate::symbols::SymbolTable;

/// Tokens that are clearly syntax or modifiers, not type names.
pub(crate) const TYPE_SYNTAX_TOKENS: &[&str] = &[
    "<",
    ">",
    ",",
    "|",
    "{",
    "}",
    "&",
    "(",
    ")",
    "[",
    "]",
    ":",
    ";",
    "=",
    "->",
    "..",
    "+",
    "-",
    "*",
    "/",
    "%",
    "!",
    "?",
    "@",
    "#",
    "==",
    "!=",
    "<=",
    ">=",
    // Modifiers and keywords that appear in type positions
    "pub",
    "ghost",
    "pure",
    "mut",
    "and",
    "or",
    "not",
    "in",
    "if",
    "then",
    "else",
    "let",
    "for",
    "forall",
    "exists",
    "old",
    "true",
    "false",
    "taint",
    "untrusted",
    "validated",
    "secret",
    "deterministic",
    "effects",
    "requires",
    "ensures",
    "invariant",
    "modifies",
    "where",
    // Self and result
    "self",
    "result",
    "Self",
];

/// Check whether a token looks like a type name candidate.
///
/// A type name is an identifier that starts with an uppercase letter and
/// is not a syntax/modifier token. We only check names that start with
/// uppercase because lowercase names are more likely to be values,
/// keywords, or effect names (e.g., `io.read`, `pure`).
pub(crate) fn is_type_name_candidate(tok: &str) -> bool {
    if TYPE_SYNTAX_TOKENS.contains(&tok) {
        return false;
    }
    // Must start with uppercase ASCII letter
    tok.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Extract candidate type names from a raw token sequence (`Vec<String>`).
///
/// Skips syntax, modifiers, and lowercase identifiers. Returns the list
/// of uppercase-initial identifiers that should resolve as types.
fn extract_type_names(tokens: &[String]) -> Vec<&str> {
    tokens
        .iter()
        .filter(|t| is_type_name_candidate(t))
        .map(|t| t.as_str())
        .collect()
}

/// Returns `true` if we should be lenient about unknown type names.
///
/// We are lenient when the file may have access to types from external
/// sources that we cannot resolve yet: unresolved imports, a project
/// declaration (which enables profiles providing types like `Region`),
/// or a module declaration (which implies a multi-module project with
/// a potential prelude). Only bare standalone files with none of these
/// get strict checking.
pub(crate) fn should_be_lenient(source: &SourceFile, imports: &[ResolvedImport]) -> bool {
    // Project declaration implies profile-provided types
    if source.project.is_some() {
        return true;
    }
    // Module declaration implies multi-module project
    if source.module.is_some() {
        return true;
    }
    // Any unresolved import means external types may exist
    imports
        .iter()
        .any(|imp| imp.status == ImportStatus::Unresolved)
}

/// Simple edit distance (Levenshtein) for "did you mean?" suggestions.
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(n + 1) {
        *val = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

/// Find the closest matching name in the symbol table for "did you mean?" hints.
/// Returns `Some("did you mean `X`?")` if a close match exists.
pub(crate) fn find_similar_name(
    name: &str,
    table: &SymbolTable,
    scope_id: usize,
) -> Option<String> {
    let threshold = match name.len() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };
    let mut best: Option<(&str, usize)> = None;
    // Walk all visible symbols from this scope upward
    let mut current = Some(scope_id);
    while let Some(id) = current {
        for sym_name in table.scopes[id].symbols.keys() {
            let dist = edit_distance(name, sym_name);
            if dist <= threshold
                && dist < name.len()
                && best.is_none_or(|(_, best_dist)| dist < best_dist)
            {
                best = Some((sym_name, dist));
            }
        }
        current = table.scopes[id].parent;
    }
    best.map(|(similar, _)| format!("did you mean `{similar}`?"))
}

/// Check a list of type-name tokens against the symbol table. Reports
/// A02001 for names that cannot be resolved. When unresolved imports
/// exist, unknown names are silently skipped (they may come from an
/// external module).
fn check_type_tokens(
    tokens: &[String],
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for name in extract_type_names(tokens) {
        if table.lookup(name, scope_id).is_some() {
            continue;
        }
        // In lenient mode (unresolved imports), skip unknown types
        if lenient {
            continue;
        }
        let suggestion = find_similar_name(name, table, scope_id);
        errors.push(ResolutionError {
            code: "A02001".into(),
            message: format!("unknown type `{name}`"),
            span: span.clone(),
            secondary: None,
            suggestion,
        });
    }
}

/// Check type references in field definitions.
fn check_fields(
    fields: &[FieldDef],
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for f in fields {
        if let Some(te) = &f.ty {
            check_type_tokens(&te.to_tokens(), table, scope_id, span, lenient, errors);
        }
    }
}

/// Check type references in function/extern parameters and return type.
fn check_fn_signature(
    params: &[Param],
    return_ty: Option<&assura_parser::ast::TypeExpr>,
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for p in params {
        if let Some(te) = &p.ty {
            check_type_tokens(&te.to_tokens(), table, scope_id, span, lenient, errors);
        }
    }
    if let Some(ret) = return_ty {
        check_type_tokens(&ret.to_tokens(), table, scope_id, span, lenient, errors);
    }
}

/// Build a map from declaration name to its scope ID by scanning the scope
/// list. When multiple scopes share a name (e.g., nested `Config` types),
/// this finds the one whose parent matches the expected parent scope.
pub(crate) fn find_scope_for(
    table: &SymbolTable,
    name: &str,
    parent_scope: usize,
) -> Option<usize> {
    // Prefer the scope whose parent matches; fall back to any match.
    let mut fallback = None;
    for (i, scope) in table.scopes.iter().enumerate() {
        if scope.name == name {
            if scope.parent == Some(parent_scope) {
                return Some(i);
            }
            if fallback.is_none() {
                fallback = Some(i);
            }
        }
    }
    fallback
}

/// Walk all declarations and resolve type references.
///
/// Uses [`DeclVisitor`] so new `Decl` variants only need a `visit_*` arm here
/// (and in `walk_decl`), not another open-coded match in this pass. Per-decl
/// walk preserves `decl.span` accuracy.
pub(crate) fn resolve_type_refs(
    source: &SourceFile,
    table: &SymbolTable,
    imports: &[ResolvedImport],
    module_scope: usize,
    errors: &mut Vec<ResolutionError>,
) {
    let lenient = should_be_lenient(source, imports);

    struct TypeRefVisitor<'a> {
        table: &'a SymbolTable,
        module_scope: usize,
        lenient: bool,
        errors: &'a mut Vec<ResolutionError>,
        decl_span: Span,
    }

    impl DeclVisitor for TypeRefVisitor<'_> {
        fn visit_type_def(&mut self, t: &TypeDef) {
            resolve_typedef_refs(
                t,
                self.table,
                &self.decl_span,
                self.module_scope,
                self.lenient,
                self.errors,
            );
        }
        fn visit_fn_def(&mut self, f: &FnDef) {
            resolve_fndef_refs(
                f,
                self.table,
                &self.decl_span,
                self.module_scope,
                self.lenient,
                self.errors,
            );
        }
        fn visit_extern(&mut self, ex: &ExternDecl) {
            resolve_extern_refs(
                ex,
                self.table,
                &self.decl_span,
                self.module_scope,
                self.lenient,
                self.errors,
            );
        }
        fn visit_bind(&mut self, b: &BindDecl) {
            // Bind has the same param/return structure as extern
            resolve_extern_refs_generic(
                &b.params,
                b.return_ty.as_ref(),
                self.table,
                &self.decl_span,
                self.module_scope,
                self.lenient,
                self.errors,
            );
        }
        fn visit_contract(&mut self, _c: &assura_parser::ast::ContractDecl) {
            // Contract clauses don't have structured type refs in
            // the current AST; nothing to check here yet.
        }
        fn visit_service(&mut self, s: &ServiceDecl) {
            let svc_scope =
                find_scope_for(self.table, &s.name, self.module_scope).unwrap_or(self.module_scope);
            for item in &s.items {
                match item {
                    ServiceItem::TypeDef(t) => {
                        resolve_typedef_refs(
                            t,
                            self.table,
                            &self.decl_span,
                            svc_scope,
                            self.lenient,
                            self.errors,
                        );
                    }
                    ServiceItem::EnumDef(e) => {
                        let enum_scope =
                            find_scope_for(self.table, &e.name, svc_scope).unwrap_or(svc_scope);
                        check_enum_variant_types(
                            e,
                            self.table,
                            enum_scope,
                            &self.decl_span,
                            self.lenient,
                            self.errors,
                        );
                    }
                    ServiceItem::States(_)
                    | ServiceItem::Operation { .. }
                    | ServiceItem::Query { .. }
                    | ServiceItem::Invariant(_)
                    | ServiceItem::Other { .. } => {}
                }
            }
        }
        fn visit_enum_def(&mut self, e: &EnumDef) {
            let enum_scope =
                find_scope_for(self.table, &e.name, self.module_scope).unwrap_or(self.module_scope);
            check_enum_variant_types(
                e,
                self.table,
                enum_scope,
                &self.decl_span,
                self.lenient,
                self.errors,
            );
        }
        // Prophecy / CodecRegistry / Block: default no-op (no structured type refs yet)
    }

    for decl in &source.decls {
        let mut visitor = TypeRefVisitor {
            table,
            module_scope,
            lenient,
            errors,
            decl_span: decl.span.clone(),
        };
        visitor.visit_decl(&decl.node);
    }
}

/// Resolve type references inside a type definition.
fn resolve_typedef_refs(
    t: &TypeDef,
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    // Use the type's own scope (which has type params) if found
    let scope = find_scope_for(table, &t.name, parent_scope).unwrap_or(parent_scope);
    match &t.body {
        TypeBody::Struct(fields) => {
            check_fields(fields, table, scope, span, lenient, errors);
        }
        TypeBody::Alias(tokens) => {
            check_type_tokens(tokens, table, scope, span, lenient, errors);
        }
        TypeBody::Refined(tokens) => {
            check_type_tokens(tokens, table, scope, span, lenient, errors);
        }
        TypeBody::Empty => {}
    }
}

/// Resolve type references in a function definition.
fn resolve_fndef_refs(
    f: &FnDef,
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    let scope = find_scope_for(table, &f.name, parent_scope).unwrap_or(parent_scope);
    check_fn_signature(
        &f.params,
        f.return_ty.as_ref(),
        table,
        scope,
        span,
        lenient,
        errors,
    );
}

/// Resolve type references in an extern function declaration.
fn resolve_extern_refs(
    ex: &ExternDecl,
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    let scope = find_scope_for(table, &ex.name, parent_scope).unwrap_or(parent_scope);
    check_fn_signature(
        &ex.params,
        ex.return_ty.as_ref(),
        table,
        scope,
        span,
        lenient,
        errors,
    );
}

fn resolve_extern_refs_generic(
    params: &[Param],
    return_ty: Option<&assura_parser::ast::TypeExpr>,
    table: &SymbolTable,
    span: &Span,
    parent_scope: usize,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    check_fn_signature(
        params,
        return_ty,
        table,
        parent_scope,
        span,
        lenient,
        errors,
    );
}

/// Check type references in enum variant fields.
///
/// Each variant has a `fields: Vec<String>` of type tokens. We check each
/// token against the symbol table using `check_type_tokens`.
fn check_enum_variant_types(
    e: &EnumDef,
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    errors: &mut Vec<ResolutionError>,
) {
    for variant in &e.variants {
        if !variant.fields.is_empty() {
            check_type_tokens(&variant.fields, table, scope_id, span, lenient, errors);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_identical() {
        assert_eq!(edit_distance("hello", "hello"), 0);
    }

    #[test]
    fn edit_distance_one_sub() {
        assert_eq!(edit_distance("cat", "car"), 1);
    }

    #[test]
    fn edit_distance_insert_delete() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn edit_distance_empty() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", ""), 0);
    }

    #[test]
    fn is_type_name_candidate_uppercase() {
        assert!(is_type_name_candidate("Int"));
        assert!(is_type_name_candidate("MyType"));
    }

    #[test]
    fn is_type_name_candidate_lowercase_rejected() {
        assert!(!is_type_name_candidate("value"));
        assert!(!is_type_name_candidate("x"));
    }

    #[test]
    fn is_type_name_candidate_syntax_tokens() {
        assert!(!is_type_name_candidate("<"));
        assert!(!is_type_name_candidate("->"));
        assert!(!is_type_name_candidate("Self"));
    }

    #[test]
    fn is_type_name_candidate_keywords() {
        assert!(!is_type_name_candidate("ghost"));
        assert!(!is_type_name_candidate("requires"));
        assert!(!is_type_name_candidate("ensures"));
    }

    #[test]
    fn extract_type_names_filters_syntax() {
        let tokens: Vec<String> = vec!["List", "<", "Int", ">"]
            .into_iter()
            .map(String::from)
            .collect();
        let names = extract_type_names(&tokens);
        assert_eq!(names, vec!["List", "Int"]);
    }

    #[test]
    fn extract_type_names_empty() {
        let names = extract_type_names(&[]);
        assert!(names.is_empty());
    }

    #[test]
    fn should_be_lenient_bare_file() {
        let source = assura_parser::parse_unwrap("");
        assert!(!should_be_lenient(&source, &[]));
    }

    #[test]
    fn should_be_lenient_with_unresolved_import() {
        let source = assura_parser::parse_unwrap("");
        let imports = vec![ResolvedImport {
            path: vec!["std".into(), "math".into()],
            items: vec![],
            alias: None,
            status: ImportStatus::Unresolved,
            span: 0..1,
        }];
        assert!(should_be_lenient(&source, &imports));
    }

    #[test]
    fn find_scope_for_not_found() {
        let table = SymbolTable::new();
        assert_eq!(find_scope_for(&table, "missing", 0), None);
    }
}
