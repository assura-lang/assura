//! Lowering pass: AST (`ResolvedFile`) -> HIR (`HirFile`).
//!
//! Walks the parsed and resolved AST, converting each declaration,
//! clause, expression, and type reference into its HIR equivalent.

use std::collections::HashMap;
use std::sync::Arc;

use assura_parser::ast::{self, Decl, Expr, ServiceItem, Spanned};
use assura_resolve::ResolvedFile;

use crate::{
    DefId, HirBind, HirBlock, HirClause, HirClauseKind, HirCodecEntry, HirCodecRegistry,
    HirContract, HirDecl, HirDeclKind, HirEnumDef, HirEnumVariant, HirExpr, HirExtern, HirFieldDef,
    HirFile, HirFnDef, HirMatchArm, HirParam, HirProphecy, HirService, HirServiceItem, HirType,
    HirTypeBody, HirTypeDef, parse_type_tokens, resolve_hir_type,
};

// ---------------------------------------------------------------------------
// Name resolution lookup
// ---------------------------------------------------------------------------

/// A helper that maps names to symbol table indices for efficient lookup.
struct NameResolver {
    /// Maps name -> symbol table index for the file scope.
    name_to_idx: HashMap<String, usize>,
}

impl NameResolver {
    fn from_resolved(resolved: &ResolvedFile) -> Self {
        let mut name_to_idx = HashMap::new();
        for (idx, sym) in resolved.symbols.symbols.iter().enumerate() {
            // First definition wins (duplicates are already reported by resolve)
            name_to_idx.entry(sym.name.clone()).or_insert(idx);
        }
        Self { name_to_idx }
    }

    fn resolve(&self, name: &str) -> DefId {
        match self.name_to_idx.get(name) {
            Some(&idx) => DefId::Resolved(idx),
            None => DefId::Unresolved(name.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Lower a `ResolvedFile` into a `HirFile`.
///
/// This is the main entry point for the lowering pass. It converts all
/// declarations, types, and expressions from AST form to HIR form,
/// resolving names to DefIds where possible.
pub fn lower(resolved: &ResolvedFile) -> HirFile {
    let resolver = NameResolver::from_resolved(resolved);
    let ctx = LowerCtx {
        resolver: &resolver,
    };

    let decls: Vec<HirDecl> = resolved
        .source
        .decls
        .iter()
        .map(|spanned| ctx.lower_decl(spanned))
        .collect();

    HirFile {
        resolved: Arc::new(resolved.clone()),
        decls,
    }
}

// ---------------------------------------------------------------------------
// Lowering context
// ---------------------------------------------------------------------------

struct LowerCtx<'a> {
    resolver: &'a NameResolver,
}

impl LowerCtx<'_> {
    // ---- Declarations ----

    fn lower_decl(&self, spanned: &Spanned<Decl>) -> HirDecl {
        let kind = match &spanned.node {
            Decl::Contract(c) => HirDeclKind::Contract(self.lower_contract(c)),
            Decl::Service(s) => HirDeclKind::Service(self.lower_service(s)),
            Decl::TypeDef(t) => HirDeclKind::TypeDef(self.lower_typedef(t)),
            Decl::EnumDef(e) => HirDeclKind::EnumDef(self.lower_enumdef(e)),
            Decl::Extern(e) => HirDeclKind::Extern(self.lower_extern(e)),
            Decl::Bind(b) => HirDeclKind::Bind(self.lower_bind(b)),
            Decl::FnDef(f) => HirDeclKind::FnDef(self.lower_fndef(f)),
            Decl::Prophecy(p) => HirDeclKind::Prophecy(self.lower_prophecy(p)),
            Decl::CodecRegistry(cr) => HirDeclKind::CodecRegistry(self.lower_codec_registry(cr)),
            Decl::Block {
                kind,
                name,
                value,
                body,
            } => HirDeclKind::Block(self.lower_block(kind, name, value, body)),
        };
        HirDecl {
            kind,
            span: spanned.span.clone(),
        }
    }

    fn lower_contract(&self, c: &ast::ContractDecl) -> HirContract {
        HirContract {
            id: self.resolver.resolve(&c.name),
            name: c.name.clone(),
            type_params: c.type_params.clone(),
            clauses: c.clauses.iter().map(|cl| self.lower_clause(cl)).collect(),
        }
    }

    fn lower_service(&self, s: &ast::ServiceDecl) -> HirService {
        HirService {
            id: self.resolver.resolve(&s.name),
            name: s.name.clone(),
            items: s
                .items
                .iter()
                .map(|item| self.lower_service_item(item))
                .collect(),
        }
    }

    fn lower_service_item(&self, item: &ServiceItem) -> HirServiceItem {
        match item {
            ServiceItem::TypeDef(t) => HirServiceItem::TypeDef(self.lower_typedef(t)),
            ServiceItem::EnumDef(e) => HirServiceItem::EnumDef(self.lower_enumdef(e)),
            ServiceItem::States(states) => HirServiceItem::States(states.clone()),
            ServiceItem::Operation { name, clauses } => HirServiceItem::Operation {
                name: name.clone(),
                clauses: clauses.iter().map(|c| self.lower_clause(c)).collect(),
            },
            ServiceItem::Query { name, clauses } => HirServiceItem::Query {
                name: name.clone(),
                clauses: clauses.iter().map(|c| self.lower_clause(c)).collect(),
            },
            ServiceItem::Invariant(expr) => HirServiceItem::Invariant(self.lower_expr(expr)),
            ServiceItem::Other { kind, body } => HirServiceItem::Other {
                kind: kind.clone(),
                body: self.lower_expr(body),
            },
        }
    }

    fn lower_typedef(&self, t: &ast::TypeDef) -> HirTypeDef {
        let body = match &t.body {
            ast::TypeBody::Alias(tokens) => HirTypeBody::Alias(parse_type_tokens(tokens)),
            ast::TypeBody::Struct(fields) => {
                HirTypeBody::Struct(fields.iter().map(|f| self.lower_field(f)).collect())
            }
            ast::TypeBody::Refined(tokens) => {
                let hir_type = parse_type_tokens(tokens);
                match hir_type {
                    HirType::Refined { base, predicate } => HirTypeBody::Refined {
                        base: *base,
                        predicate,
                    },
                    other => HirTypeBody::Alias(other),
                }
            }
            ast::TypeBody::Empty => HirTypeBody::Empty,
        };
        HirTypeDef {
            id: self.resolver.resolve(&t.name),
            name: t.name.clone(),
            type_params: t.type_params.clone(),
            body,
        }
    }

    fn lower_field(&self, f: &ast::FieldDef) -> HirFieldDef {
        HirFieldDef {
            name: f.name.clone(),
            ty: resolve_hir_type(f.parsed_type.as_ref(), &f.ty),
            is_pub: f.is_pub,
        }
    }

    fn lower_enumdef(&self, e: &ast::EnumDef) -> HirEnumDef {
        HirEnumDef {
            id: self.resolver.resolve(&e.name),
            name: e.name.clone(),
            type_params: e.type_params.clone(),
            variants: e
                .variants
                .iter()
                .map(|v| HirEnumVariant {
                    name: v.name.clone(),
                    fields: v
                        .fields
                        .iter()
                        .map(|f| parse_type_tokens(std::slice::from_ref(f)))
                        .collect(),
                })
                .collect(),
        }
    }

    fn lower_extern(&self, e: &ast::ExternDecl) -> HirExtern {
        HirExtern {
            id: self.resolver.resolve(&e.name),
            name: e.name.clone(),
            params: e.params.iter().map(|p| self.lower_param(p)).collect(),
            return_ty: resolve_hir_type(e.return_type_expr.as_ref(), &e.return_ty),
            clauses: e.clauses.iter().map(|c| self.lower_clause(c)).collect(),
        }
    }

    fn lower_bind(&self, b: &ast::BindDecl) -> HirBind {
        HirBind {
            id: self.resolver.resolve(&b.name),
            name: b.name.clone(),
            target_path: b.target_path.clone(),
            params: b.params.iter().map(|p| self.lower_param(p)).collect(),
            return_ty: resolve_hir_type(b.return_type_expr.as_ref(), &b.return_ty),
            clauses: b.clauses.iter().map(|c| self.lower_clause(c)).collect(),
        }
    }

    fn lower_fndef(&self, f: &ast::FnDef) -> HirFnDef {
        HirFnDef {
            id: self.resolver.resolve(&f.name),
            name: f.name.clone(),
            is_ghost: f.is_ghost,
            is_lemma: f.is_lemma,
            params: f.params.iter().map(|p| self.lower_param(p)).collect(),
            return_ty: resolve_hir_type(f.return_type_expr.as_ref(), &f.return_ty),
            clauses: f.clauses.iter().map(|c| self.lower_clause(c)).collect(),
        }
    }

    fn lower_param(&self, p: &ast::Param) -> HirParam {
        HirParam {
            name: p.name.clone(),
            ty: resolve_hir_type(p.parsed_type.as_ref(), &p.ty),
        }
    }

    fn lower_prophecy(&self, p: &ast::ProphecyDecl) -> HirProphecy {
        let ty = if p.ty_tokens.is_empty() {
            HirType::Unit
        } else {
            parse_type_tokens(&p.ty_tokens)
        };
        HirProphecy {
            id: self.resolver.resolve(&p.name),
            name: p.name.clone(),
            ty,
        }
    }

    fn lower_codec_registry(&self, cr: &ast::CodecRegistryDecl) -> HirCodecRegistry {
        HirCodecRegistry {
            id: self.resolver.resolve(&cr.name),
            name: cr.name.clone(),
            output_type: cr.output_type.clone(),
            codecs: cr
                .codecs
                .iter()
                .map(|c| HirCodecEntry {
                    name: c.name.clone(),
                    magic: c.magic.clone(),
                    decoder: c.decoder.clone(),
                    contracts: c.contracts.iter().map(|cl| self.lower_clause(cl)).collect(),
                })
                .collect(),
        }
    }

    fn lower_block(
        &self,
        kind: &str,
        name: &str,
        value: &Option<Vec<String>>,
        body: &[ast::Clause],
    ) -> HirBlock {
        HirBlock {
            kind: kind.to_string(),
            name: name.to_string(),
            value: value
                .as_ref()
                .map(|tokens| HirExpr::RawTokens(tokens.clone())),
            clauses: body.iter().map(|c| self.lower_clause(c)).collect(),
        }
    }

    // ---- Clauses ----

    fn lower_clause(&self, clause: &ast::Clause) -> HirClause {
        HirClause {
            kind: HirClauseKind::from(&clause.kind),
            body: self.lower_expr(&clause.body),
        }
    }

    // ---- Expressions ----

    fn lower_expr(&self, expr: &Expr) -> HirExpr {
        match expr {
            Expr::Literal(lit) => HirExpr::Literal(lit.clone()),
            Expr::Ident(name) => HirExpr::Ident {
                name: name.clone(),
                def_id: Some(self.resolver.resolve(name)),
            },
            Expr::Field(base, field) => {
                HirExpr::Field(Box::new(self.lower_expr(base)), field.clone())
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => HirExpr::MethodCall {
                receiver: Box::new(self.lower_expr(receiver)),
                method: method.clone(),
                args: args.iter().map(|a| self.lower_expr(a)).collect(),
            },
            Expr::Call { func, args } => HirExpr::Call {
                func: Box::new(self.lower_expr(func)),
                args: args.iter().map(|a| self.lower_expr(a)).collect(),
            },
            Expr::Index { expr, index } => HirExpr::Index {
                expr: Box::new(self.lower_expr(expr)),
                index: Box::new(self.lower_expr(index)),
            },
            Expr::BinOp { lhs, op, rhs } => HirExpr::BinOp {
                lhs: Box::new(self.lower_expr(lhs)),
                op: op.clone(),
                rhs: Box::new(self.lower_expr(rhs)),
            },
            Expr::UnaryOp { op, expr } => HirExpr::UnaryOp {
                op: op.clone(),
                expr: Box::new(self.lower_expr(expr)),
            },
            Expr::Old(e) => HirExpr::Old(Box::new(self.lower_expr(e))),
            Expr::Forall { var, domain, body } => HirExpr::Forall {
                var: var.clone(),
                domain: Box::new(self.lower_expr(domain)),
                body: Box::new(self.lower_expr(body)),
            },
            Expr::Exists { var, domain, body } => HirExpr::Exists {
                var: var.clone(),
                domain: Box::new(self.lower_expr(domain)),
                body: Box::new(self.lower_expr(body)),
            },
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => HirExpr::If {
                cond: Box::new(self.lower_expr(cond)),
                then_branch: Box::new(self.lower_expr(then_branch)),
                else_branch: else_branch.as_ref().map(|e| Box::new(self.lower_expr(e))),
            },
            Expr::Paren(e) => HirExpr::Paren(Box::new(self.lower_expr(e))),
            Expr::List(items) => HirExpr::List(items.iter().map(|i| self.lower_expr(i)).collect()),
            Expr::Cast { expr, ty } => HirExpr::Cast {
                expr: Box::new(self.lower_expr(expr)),
                ty: ty.clone(),
            },
            Expr::Block(items) => {
                HirExpr::Block(items.iter().map(|i| self.lower_expr(i)).collect())
            }
            Expr::Ghost(e) => HirExpr::Ghost(Box::new(self.lower_expr(e))),
            Expr::Apply { lemma_name, args } => HirExpr::Apply {
                lemma_name: lemma_name.clone(),
                args: args.iter().map(|a| self.lower_expr(a)).collect(),
            },
            Expr::Let { name, value, body } => HirExpr::Let {
                name: name.clone(),
                value: Box::new(self.lower_expr(value)),
                body: Box::new(self.lower_expr(body)),
            },
            Expr::Match { scrutinee, arms } => HirExpr::Match {
                scrutinee: Box::new(self.lower_expr(scrutinee)),
                arms: arms
                    .iter()
                    .map(|a| HirMatchArm {
                        pattern: a.pattern.clone(),
                        body: self.lower_expr(&a.body),
                    })
                    .collect(),
            },
            Expr::Tuple(items) => {
                HirExpr::Tuple(items.iter().map(|i| self.lower_expr(i)).collect())
            }
            // Raw tokens: preserved for non-expression clause bodies
            // (effects, modifies, input, output). These are intentionally
            // raw because they contain token lists, not expressions.
            Expr::Raw(tokens) => HirExpr::RawTokens(tokens.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::*;

    fn make_resolved(source: SourceFile) -> ResolvedFile {
        assura_resolve::resolve(&source).unwrap()
    }

    #[test]
    fn lower_empty_file() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        assert!(hir.decls.is_empty());
    }

    #[test]
    fn lower_contract_with_requires() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::Contract(ContractDecl {
                    name: "Foo".into(),
                    type_params: vec![],
                    clauses: vec![Clause {
                        kind: ClauseKind::Requires,
                        body: Expr::BinOp {
                            lhs: Box::new(Expr::Ident("x".into())),
                            op: BinOp::Gt,
                            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                        },
                    }],
                }),
                span: 0..10,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        assert_eq!(hir.decls.len(), 1);
        if let HirDeclKind::Contract(c) = &hir.decls[0].kind {
            assert_eq!(c.name, "Foo");
            assert_eq!(c.clauses.len(), 1);
            assert_eq!(c.clauses[0].kind, HirClauseKind::Requires);
            // Name should be resolved
            assert!(matches!(&c.id, DefId::Resolved(_)));
        } else {
            panic!("expected Contract declaration");
        }
    }

    #[test]
    fn lower_fn_resolves_name() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::FnDef(FnDef {
                    name: "helper".into(),
                    is_ghost: false,
                    is_lemma: false,
                    params: vec![Param {
                        name: "n".into(),
                        ty: vec!["Int".into()],
                        parsed_type: None,
                    }],
                    return_ty: vec!["Bool".into()],
                    return_type_expr: None,
                    clauses: vec![],
                }),
                span: 0..20,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        assert_eq!(hir.decls.len(), 1);
        if let HirDeclKind::FnDef(f) = &hir.decls[0].kind {
            assert_eq!(f.name, "helper");
            assert!(matches!(f.id, DefId::Resolved(_)));
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].ty, crate::HirType::Named("Int".into()));
            assert_eq!(f.return_ty, crate::HirType::Named("Bool".into()));
        } else {
            panic!("expected FnDef");
        }
    }

    #[test]
    fn lower_typedef_struct() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::TypeDef(TypeDef {
                    name: "Point".into(),
                    type_params: vec![],
                    body: TypeBody::Struct(vec![
                        FieldDef {
                            name: "x".into(),
                            ty: vec!["Int".into()],
                            parsed_type: None,
                            is_pub: true,
                        },
                        FieldDef {
                            name: "y".into(),
                            ty: vec!["Int".into()],
                            parsed_type: None,
                            is_pub: true,
                        },
                    ]),
                }),
                span: 0..30,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        if let HirDeclKind::TypeDef(t) = &hir.decls[0].kind {
            assert_eq!(t.name, "Point");
            if let HirTypeBody::Struct(fields) = &t.body {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "x");
                assert_eq!(fields[0].ty, crate::HirType::Named("Int".into()));
            } else {
                panic!("expected Struct body");
            }
        } else {
            panic!("expected TypeDef");
        }
    }

    #[test]
    fn lower_expr_ident_resolved() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::Contract(ContractDecl {
                    name: "Test".into(),
                    type_params: vec![],
                    clauses: vec![Clause {
                        kind: ClauseKind::Requires,
                        body: Expr::Ident("Test".into()),
                    }],
                }),
                span: 0..5,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        if let HirDeclKind::Contract(c) = &hir.decls[0].kind {
            if let HirExpr::Ident { name, def_id } = &c.clauses[0].body {
                assert_eq!(name, "Test");
                assert!(def_id.is_some());
                assert!(matches!(def_id, Some(DefId::Resolved(_))));
            } else {
                panic!("expected Ident");
            }
        }
    }

    #[test]
    fn lower_demo_file_roundtrip() {
        // Parse a real demo file and verify lowering + to_ast_expr roundtrip
        let source_text = r#"
contract SafeDivision {
    input(a: Int, b: Int)
    output(result: Int)
    requires { b != 0 }
    ensures { result * b + (a % b) == a }
}
"#;
        let (file, errors) = assura_parser::parse(source_text);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let file = file.unwrap();
        let resolved = make_resolved(file);
        let hir = lower(&resolved);

        // Verify lowering produced the contract
        assert_eq!(hir.decls.len(), 1);
        if let HirDeclKind::Contract(c) = &hir.decls[0].kind {
            assert_eq!(c.name, "SafeDivision");
            assert!(c.clauses.len() >= 3);

            // Verify round-trip: each HirClause -> ast::Clause
            for clause in &c.clauses {
                let _ast_clause = clause.to_ast_clause();
                // Should not panic
            }
        }

        // Verify to_source_file gives back the original
        let source_file = hir.to_source_file();
        assert_eq!(source_file.decls.len(), 1);
    }

    #[test]
    fn lower_extern_with_return_type() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::Extern(ExternDecl {
                    name: "malloc".into(),
                    params: vec![Param {
                        name: "size".into(),
                        ty: vec!["Nat".into()],
                        parsed_type: None,
                    }],
                    return_ty: vec!["Bytes".into()],
                    return_type_expr: None,
                    clauses: vec![],
                }),
                span: 0..20,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        if let HirDeclKind::Extern(e) = &hir.decls[0].kind {
            assert_eq!(e.name, "malloc");
            assert_eq!(e.return_ty, crate::HirType::Named("Bytes".into()));
            assert_eq!(e.params[0].ty, crate::HirType::Named("Nat".into()));
        } else {
            panic!("expected Extern");
        }
    }

    #[test]
    fn lower_service_with_states() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::Service(ServiceDecl {
                    name: "Connection".into(),
                    items: vec![
                        ServiceItem::States(vec!["Open".into(), "Closed".into()]),
                        ServiceItem::Operation {
                            name: "Close".into(),
                            clauses: vec![],
                        },
                    ],
                }),
                span: 0..30,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        if let HirDeclKind::Service(s) = &hir.decls[0].kind {
            assert_eq!(s.name, "Connection");
            assert_eq!(s.items.len(), 2);
            assert!(matches!(&s.items[0], HirServiceItem::States(s) if s.len() == 2));
        } else {
            panic!("expected Service");
        }
    }

    #[test]
    fn lower_enum_def() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::EnumDef(EnumDef {
                    name: "Color".into(),
                    type_params: vec![],
                    variants: vec![
                        ast::EnumVariant {
                            name: "Red".into(),
                            fields: vec![],
                        },
                        ast::EnumVariant {
                            name: "Green".into(),
                            fields: vec![],
                        },
                    ],
                }),
                span: 0..20,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        if let HirDeclKind::EnumDef(e) = &hir.decls[0].kind {
            assert_eq!(e.name, "Color");
            assert_eq!(e.variants.len(), 2);
            assert_eq!(e.variants[0].name, "Red");
        } else {
            panic!("expected EnumDef");
        }
    }

    #[test]
    fn lower_block_decl() {
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::Block {
                    kind: "feature_max".into(),
                    name: "MAX_SIZE".into(),
                    value: Some(vec!["1024".into()]),
                    body: vec![],
                },
                span: 0..20,
            }],
        };
        let resolved = make_resolved(source);
        let hir = lower(&resolved);
        if let HirDeclKind::Block(b) = &hir.decls[0].kind {
            assert_eq!(b.kind, "feature_max");
            assert_eq!(b.name, "MAX_SIZE");
            assert!(matches!(&b.value, Some(HirExpr::RawTokens(t)) if t == &["1024"]));
        } else {
            panic!("expected Block");
        }
    }
}
