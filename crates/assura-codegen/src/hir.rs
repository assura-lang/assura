//! Typed Rust HIR (High-level Intermediate Representation) for codegen.
//!
//! Instead of building Rust source code via string concatenation,
//! codegen builds structured `RustItem` / `RustStmt` / `RustExpr` trees.
//! These are converted to `syn` AST nodes, then formatted by `prettyplease`.
//!
//! Benefits:
//! - Structural validation: malformed HIR caught at construction, not formatting.
//! - Testability: assert on HIR structure (e.g., "2 Assert nodes"), not strings.
//! - No string-escaping bugs: identifiers and types are typed, not interpolated.

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Core HIR types
// ---------------------------------------------------------------------------

/// A top-level Rust item (function, struct, enum, trait, const, mod, etc.).
#[derive(Debug, Clone, PartialEq)]
pub enum RustItem {
    /// `pub fn name(params) -> ret { body }`
    Fn(RustFn),
    /// `pub struct Name { fields }` or `pub struct Name;`
    Struct(RustStruct),
    /// `pub enum Name { variants }`
    Enum(RustEnum),
    /// `pub trait Name { methods }`
    Trait(RustTrait),
    /// `impl Trait for Type { methods }`
    Impl(RustImpl),
    /// `pub mod name { items }`
    Mod(RustMod),
    /// `pub const NAME: Type = value;`
    Const(RustConst),
    /// `use path;`
    Use(String),
    /// `// comment` or `/// doc comment`
    Comment(String),
    /// An attribute like `#[allow(...)]` or `#![allow(...)]` applied to the crate/file.
    InnerAttr(String),
    /// Raw Rust code string (escape hatch for migration).
    ///
    /// Used during incremental migration: parts of codegen not yet converted
    /// to HIR can emit raw strings that are spliced into the output.
    Raw(String),
}

/// A Rust function definition.
#[derive(Debug, Clone, PartialEq)]
pub struct RustFn {
    /// Function name.
    pub name: String,
    /// Generic type parameters (e.g., `["T", "U"]`).
    pub type_params: Vec<String>,
    /// Function parameters.
    pub params: Vec<RustParam>,
    /// Return type. `None` means no explicit return type (unit).
    pub ret: Option<RustType>,
    /// Function body statements.
    pub body: Vec<RustStmt>,
    /// Whether the function is `pub`.
    pub is_pub: bool,
    /// Whether the function is `unsafe`.
    pub is_unsafe: bool,
    /// Doc comments (each string is one `///` line).
    pub doc: Vec<String>,
    /// Attributes (e.g., `#[allow(dead_code)]`).
    pub attrs: Vec<String>,
    /// Whether this is an abstract method declaration (no body, e.g., trait methods).
    pub is_abstract: bool,
}

impl Default for RustFn {
    fn default() -> Self {
        Self {
            name: String::new(),
            type_params: Vec::new(),
            params: Vec::new(),
            ret: None,
            body: Vec::new(),
            is_pub: true,
            is_unsafe: false,
            doc: Vec::new(),
            attrs: Vec::new(),
            is_abstract: false,
        }
    }
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct RustParam {
    pub name: String,
    pub ty: RustType,
}

/// A Rust type.
#[derive(Debug, Clone, PartialEq)]
pub enum RustType {
    /// A simple named type: `i64`, `String`, `MyStruct`.
    Named(String),
    /// A generic type: `Vec<u8>`, `Result<T, E>`.
    Generic(String, Vec<RustType>),
    /// A reference: `&T` or `&[u8]`.
    Ref(Box<RustType>),
    /// A mutable reference: `&mut T`.
    RefMut(Box<RustType>),
    /// A tuple type: `(A, B, C)`.
    Tuple(Vec<RustType>),
    /// Unit type: `()`.
    Unit,
    /// Never type: `!`.
    Never,
    /// A raw type string (escape hatch for complex types not yet modeled).
    Raw(String),
}

impl RustType {
    /// Shorthand for common types.
    pub fn i64() -> Self {
        Self::Named("i64".into())
    }
    pub fn u64() -> Self {
        Self::Named("u64".into())
    }
    pub fn f64() -> Self {
        Self::Named("f64".into())
    }
    pub fn bool() -> Self {
        Self::Named("bool".into())
    }
    pub fn string() -> Self {
        Self::Named("String".into())
    }
    pub fn bytes() -> Self {
        Self::Generic("Vec".into(), vec![Self::Named("u8".into())])
    }
    pub fn result(ok: RustType, err: RustType) -> Self {
        Self::Generic("Result".into(), vec![ok, err])
    }
}

/// A Rust statement inside a function body.
#[derive(Debug, Clone, PartialEq)]
pub enum RustStmt {
    /// `let name: ty = init;` or `let name = init;`
    Let {
        name: String,
        ty: Option<RustType>,
        init: RustExpr,
    },
    /// `debug_assert!(cond, "label: msg");`
    Assert { cond: String, label: String },
    /// A `// comment` line.
    Comment(String),
    /// An expression used as a statement (e.g., `x;` or `x` as trailing expr).
    Expr(RustExpr),
    /// Raw Rust code (escape hatch during migration).
    Raw(String),
}

/// A Rust expression.
#[derive(Debug, Clone, PartialEq)]
pub enum RustExpr {
    /// An identifier: `x`, `RESULT_VAR`.
    Ident(String),
    /// A literal: `42`, `"hello"`, `true`.
    Literal(String),
    /// A function/method call: `func(args)`.
    Call(String, Vec<RustExpr>),
    /// A method call: `receiver.method(args)`.
    MethodCall(Box<RustExpr>, String, Vec<RustExpr>),
    /// `todo!("msg")`.
    Todo(String),
    /// `Ok(expr)`.
    Ok(Box<RustExpr>),
    /// `.clone()` on an expression.
    Clone(Box<RustExpr>),
    /// Raw Rust expression string (escape hatch).
    Raw(String),
}

/// A Rust struct definition.
#[derive(Debug, Clone, PartialEq)]
pub struct RustStruct {
    pub name: String,
    pub type_params: Vec<String>,
    pub fields: Vec<RustField>,
    /// Derive attributes (e.g., `["Debug", "Clone", "PartialEq"]`).
    pub derives: Vec<String>,
    pub is_pub: bool,
    /// Doc comments.
    pub doc: Vec<String>,
}

impl Default for RustStruct {
    fn default() -> Self {
        Self {
            name: String::new(),
            type_params: Vec::new(),
            fields: Vec::new(),
            derives: vec!["Debug".into(), "Clone".into(), "PartialEq".into()],
            is_pub: true,
            doc: Vec::new(),
        }
    }
}

/// A struct field.
#[derive(Debug, Clone, PartialEq)]
pub struct RustField {
    pub name: String,
    pub ty: RustType,
    pub is_pub: bool,
}

/// A Rust enum definition.
#[derive(Debug, Clone, PartialEq)]
pub struct RustEnum {
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<RustVariant>,
    pub derives: Vec<String>,
    pub is_pub: bool,
    pub doc: Vec<String>,
    /// Extra attributes beyond derives (e.g., `#[non_exhaustive]`).
    pub attrs: Vec<String>,
}

impl Default for RustEnum {
    fn default() -> Self {
        Self {
            name: String::new(),
            type_params: Vec::new(),
            variants: Vec::new(),
            derives: vec!["Debug".into(), "Clone".into(), "PartialEq".into()],
            is_pub: true,
            doc: Vec::new(),
            attrs: Vec::new(),
        }
    }
}

/// An enum variant.
#[derive(Debug, Clone, PartialEq)]
pub struct RustVariant {
    pub name: String,
    /// Tuple fields (empty for unit variants).
    pub fields: Vec<RustType>,
    /// Extra attributes (e.g., `#[error("...")]` for thiserror).
    pub attrs: Vec<String>,
}

/// A Rust trait definition.
#[derive(Debug, Clone, PartialEq)]
pub struct RustTrait {
    pub name: String,
    pub type_params: Vec<String>,
    pub supertraits: Vec<String>,
    pub methods: Vec<RustFn>,
    pub is_pub: bool,
    pub doc: Vec<String>,
}

/// An `impl` block.
#[derive(Debug, Clone, PartialEq)]
pub struct RustImpl {
    /// The trait being implemented (e.g., `"Display"`). `None` for inherent impls.
    pub trait_name: Option<String>,
    /// The type the impl is for (e.g., `"MyStruct"`).
    pub target: String,
    pub type_params: Vec<String>,
    pub methods: Vec<RustFn>,
}

/// A module definition.
#[derive(Debug, Clone, PartialEq)]
pub struct RustMod {
    pub name: String,
    pub items: Vec<RustItem>,
    pub is_pub: bool,
    pub doc: Vec<String>,
}

/// A constant definition.
#[derive(Debug, Clone, PartialEq)]
pub struct RustConst {
    pub name: String,
    pub ty: RustType,
    pub value: String,
    pub is_pub: bool,
    pub doc: Vec<String>,
}

// ---------------------------------------------------------------------------
// HIR -> String rendering (via syn + prettyplease)
// ---------------------------------------------------------------------------

/// Render a list of `RustItem`s to formatted Rust source code.
///
/// Converts each item to its string representation, joins them,
/// then formats the result via `syn::parse_file` + `prettyplease::unparse`.
pub fn render_items(items: &[RustItem]) -> String {
    let mut code = String::new();
    for item in items {
        render_item(item, &mut code);
    }
    // Format via prettyplease (same path as the existing format_rust)
    match syn::parse_file(&code) {
        Ok(syntax_tree) => prettyplease::unparse(&syntax_tree),
        Err(e) => {
            eprintln!("warning: HIR-generated Rust has syntax errors, skipping formatting: {e}");
            format!("// WARNING: prettyplease formatting skipped (parse error: {e})\n\n{code}")
        }
    }
}

/// Render a single `RustItem` to a string, appending to `out`.
fn render_item(item: &RustItem, out: &mut String) {
    match item {
        RustItem::Fn(f) => render_fn(f, out),
        RustItem::Struct(s) => render_struct(s, out),
        RustItem::Enum(e) => render_enum(e, out),
        RustItem::Trait(t) => render_trait(t, out),
        RustItem::Impl(i) => render_impl(i, out),
        RustItem::Mod(m) => render_mod(m, out),
        RustItem::Const(c) => render_const(c, out),
        RustItem::Use(path) => {
            let _ = writeln!(out, "use {path};");
        }
        RustItem::Comment(text) => {
            let _ = writeln!(out, "// {text}");
        }
        RustItem::InnerAttr(attr) => {
            let _ = writeln!(out, "#![{attr}]");
        }
        RustItem::Raw(code) => {
            out.push_str(code);
            if !code.ends_with('\n') {
                out.push('\n');
            }
        }
    }
}

fn render_fn(f: &RustFn, out: &mut String) {
    render_fn_opts(f, out, &RenderOpts::default());
}

fn render_fn_opts(f: &RustFn, out: &mut String, opts: &RenderOpts) {
    for doc in &f.doc {
        let _ = writeln!(out, "/// {doc}");
    }
    for attr in &f.attrs {
        let _ = writeln!(out, "#[{attr}]");
    }

    let vis = if f.is_pub { "pub " } else { "" };
    let unsafe_kw = if f.is_unsafe { "unsafe " } else { "" };
    let tps = if f.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", f.type_params.join(", "))
    };

    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            // Self receiver params are rendered without an explicit type
            if p.name == "&self" || p.name == "self" || p.name == "&mut self" {
                p.name.clone()
            } else {
                format!("{}: {}", p.name, render_type(&p.ty))
            }
        })
        .collect();
    let params_s = params.join(", ");

    let ret = match &f.ret {
        Some(ty) => format!(" -> {}", render_type(ty)),
        None => String::new(),
    };

    if f.is_abstract {
        let _ = writeln!(
            out,
            "{vis}{unsafe_kw}fn {}{tps}({params_s}){ret};\n",
            f.name
        );
    } else {
        let _ = writeln!(
            out,
            "{vis}{unsafe_kw}fn {}{tps}({params_s}){ret} {{",
            f.name
        );

        for stmt in &f.body {
            render_stmt_opts(stmt, out, 1, opts);
        }

        out.push_str("}\n\n");
    }
}

fn render_struct(s: &RustStruct, out: &mut String) {
    for doc in &s.doc {
        let _ = writeln!(out, "/// {doc}");
    }
    if !s.derives.is_empty() {
        let _ = writeln!(out, "#[derive({})]", s.derives.join(", "));
    }

    let vis = if s.is_pub { "pub " } else { "" };
    let tps = if s.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", s.type_params.join(", "))
    };

    if s.fields.is_empty() {
        let _ = writeln!(out, "{vis}struct {}{tps};\n", s.name);
    } else {
        let _ = writeln!(out, "{vis}struct {}{tps} {{", s.name);
        for field in &s.fields {
            let fvis = if field.is_pub { "pub " } else { "" };
            let _ = writeln!(out, "    {fvis}{}: {},", field.name, render_type(&field.ty));
        }
        out.push_str("}\n\n");
    }
}

fn render_enum(e: &RustEnum, out: &mut String) {
    for doc in &e.doc {
        let _ = writeln!(out, "/// {doc}");
    }
    if !e.derives.is_empty() {
        let _ = writeln!(out, "#[derive({})]", e.derives.join(", "));
    }
    for attr in &e.attrs {
        let _ = writeln!(out, "#[{attr}]");
    }

    let vis = if e.is_pub { "pub " } else { "" };
    let tps = if e.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", e.type_params.join(", "))
    };

    let _ = writeln!(out, "{vis}enum {}{tps} {{", e.name);
    for variant in &e.variants {
        for attr in &variant.attrs {
            let _ = writeln!(out, "    #[{attr}]");
        }
        if variant.fields.is_empty() {
            let _ = writeln!(out, "    {},", variant.name);
        } else {
            let fields: Vec<String> = variant.fields.iter().map(render_type).collect();
            let _ = writeln!(out, "    {}({}),", variant.name, fields.join(", "));
        }
    }
    out.push_str("}\n\n");
}

fn render_trait(t: &RustTrait, out: &mut String) {
    for doc in &t.doc {
        let _ = writeln!(out, "/// {doc}");
    }

    let vis = if t.is_pub { "pub " } else { "" };
    let tps = if t.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", t.type_params.join(", "))
    };
    let bounds = if t.supertraits.is_empty() {
        String::new()
    } else {
        format!(": {}", t.supertraits.join(" + "))
    };

    let _ = writeln!(out, "{vis}trait {}{tps}{bounds} {{", t.name);
    for method in &t.methods {
        // Render as a trait method (with or without default body)
        render_fn(method, out);
    }
    out.push_str("}\n\n");
}

fn render_impl(i: &RustImpl, out: &mut String) {
    let tps = if i.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", i.type_params.join(", "))
    };

    match &i.trait_name {
        Some(trait_name) => {
            let _ = writeln!(out, "impl{tps} {trait_name} for {}{tps} {{", i.target);
        }
        None => {
            let _ = writeln!(out, "impl{tps} {}{tps} {{", i.target);
        }
    }
    for method in &i.methods {
        render_fn(method, out);
    }
    out.push_str("}\n\n");
}

fn render_mod(m: &RustMod, out: &mut String) {
    for doc in &m.doc {
        let _ = writeln!(out, "/// {doc}");
    }
    let vis = if m.is_pub { "pub " } else { "" };
    let _ = writeln!(out, "{vis}mod {} {{", m.name);
    for item in &m.items {
        render_item(item, out);
    }
    out.push_str("}\n\n");
}

fn render_const(c: &RustConst, out: &mut String) {
    for doc in &c.doc {
        let _ = writeln!(out, "/// {doc}");
    }
    let vis = if c.is_pub { "pub " } else { "" };
    let _ = writeln!(
        out,
        "{vis}const {}: {} = {};",
        c.name,
        render_type(&c.ty),
        c.value
    );
}

fn render_type(ty: &RustType) -> String {
    match ty {
        RustType::Named(name) => name.clone(),
        RustType::Generic(name, args) => {
            let args_s: Vec<String> = args.iter().map(render_type).collect();
            format!("{name}<{}>", args_s.join(", "))
        }
        RustType::Ref(inner) => format!("&{}", render_type(inner)),
        RustType::RefMut(inner) => format!("&mut {}", render_type(inner)),
        RustType::Tuple(items) => {
            let items_s: Vec<String> = items.iter().map(render_type).collect();
            format!("({})", items_s.join(", "))
        }
        RustType::Unit => "()".into(),
        RustType::Never => "!".into(),
        RustType::Raw(s) => s.clone(),
    }
}

#[cfg(test)]
pub(crate) fn render_stmt(stmt: &RustStmt, out: &mut String, indent: usize) {
    render_stmt_opts(stmt, out, indent, &RenderOpts::default());
}

/// Options that control how statements are rendered.
#[derive(Debug, Clone, Default)]
pub(crate) struct RenderOpts {
    /// When true, assertions emit `assura_runtime::contract_violation` calls
    /// that persist in release builds instead of `debug_assert!`.
    pub runtime_checks: bool,
    /// The contract name for runtime violation reports.
    pub contract_name: String,
}

pub(crate) fn render_stmt_opts(
    stmt: &RustStmt,
    out: &mut String,
    indent: usize,
    opts: &RenderOpts,
) {
    let pad = "    ".repeat(indent);
    match stmt {
        RustStmt::Let { name, ty, init } => {
            let ty_s = match ty {
                Some(t) => format!(": {}", render_type(t)),
                None => String::new(),
            };
            let _ = writeln!(out, "{pad}let {name}{ty_s} = {};", render_expr(init));
        }
        RustStmt::Assert { cond, label } => {
            // If expression references deep field chains (e.g., state.head.extra),
            // emit as a comment since stub types don't have these fields.
            if crate::has_deep_field_access(cond) {
                let _ = writeln!(out, "{pad}// {label}: {}", cond.replace('"', "\\\""));
            } else if opts.runtime_checks {
                // Runtime checks: persist in release builds via assura_runtime
                let escaped_cond = cond.replace('"', "\\\"");
                let contract = opts.contract_name.replace('"', "\\\"");
                if cond.contains('\n') {
                    let flat_cond = cond.replace('\n', " ");
                    let _ = writeln!(
                        out,
                        "{pad}if !({{ {cond} }}) {{ assura_runtime::contract_violation(\"{contract}\", \"{label}\", \"{escaped_cond}\", file!(), line!()); }}"
                    );
                    let _ = flat_cond.len(); // suppress unused warning
                } else {
                    let _ = writeln!(
                        out,
                        "{pad}if !({cond}) {{ assura_runtime::contract_violation(\"{contract}\", \"{label}\", \"{escaped_cond}\", file!(), line!()); }}"
                    );
                }
            } else if cond.contains('\n') {
                let msg = cond
                    .replace('\n', " ")
                    .replace('"', "\\\"")
                    .replace('{', "{{")
                    .replace('}', "}}");
                let _ = writeln!(out, "{pad}debug_assert!({{ {cond} }}, \"{label}: {msg}\");");
            } else {
                // Escape braces so `if { } else { }` ensures do not break the
                // format string used to build the debug_assert! source.
                let escaped_cond = cond
                    .replace('"', "\\\"")
                    .replace('{', "{{")
                    .replace('}', "}}");
                let _ = writeln!(
                    out,
                    "{pad}debug_assert!({cond}, \"{label}: {escaped_cond}\");"
                );
            }
        }
        RustStmt::Comment(text) => {
            let _ = writeln!(out, "{pad}// {text}");
        }
        RustStmt::Expr(expr) => {
            let _ = writeln!(out, "{pad}{}", render_expr(expr));
        }
        RustStmt::Raw(code) => {
            for line in code.lines() {
                let _ = writeln!(out, "{pad}{line}");
            }
        }
    }
}

fn render_expr(expr: &RustExpr) -> String {
    match expr {
        RustExpr::Ident(name) => name.clone(),
        RustExpr::Literal(lit) => lit.clone(),
        RustExpr::Call(func, args) => {
            let args_s: Vec<String> = args.iter().map(render_expr).collect();
            format!("{func}({})", args_s.join(", "))
        }
        RustExpr::MethodCall(receiver, method, args) => {
            let args_s: Vec<String> = args.iter().map(render_expr).collect();
            format!("{}.{method}({})", render_expr(receiver), args_s.join(", "))
        }
        RustExpr::Todo(msg) => format!("todo!(\"{msg}\")"),
        RustExpr::Ok(inner) => format!("Ok({})", render_expr(inner)),
        RustExpr::Clone(inner) => format!("{}.clone()", render_expr(inner)),
        RustExpr::Raw(code) => code.clone(),
    }
}

// ---------------------------------------------------------------------------
// Builder helpers for common codegen patterns
// ---------------------------------------------------------------------------

/// Build a `#[derive(Debug, thiserror::Error)]` enum for contract error types.
///
/// Each variant gets a `#[error("VariantName")]` attribute for Display impl.
pub fn build_error_enum(contract_name: &str, variants: &[String]) -> RustItem {
    let enum_name = format!("{contract_name}Error");
    RustItem::Enum(RustEnum {
        name: enum_name,
        variants: variants
            .iter()
            .map(|v| RustVariant {
                name: v.clone(),
                fields: vec![],
                attrs: vec![format!("error(\"{v}\")")],
            })
            .collect(),
        derives: vec!["Debug".into(), "thiserror::Error".into()],
        ..RustEnum::default()
    })
}

/// Build a `#[derive(Debug, Clone, PartialEq)]` enum with optional Display impl
/// and exhaustiveness check function.
///
/// Returns a list of items: the enum definition, optionally a Display impl,
/// and optionally an exhaustiveness check function.
pub fn build_enum_def(e: &assura_ast::EnumDef) -> Vec<RustItem> {
    let mut items = Vec::new();

    let tps: Vec<String> = e.type_params.clone();

    // Build enum
    let rust_enum = RustEnum {
        name: e.name.clone(),
        type_params: tps.clone(),
        variants: e
            .variants
            .iter()
            .map(|v| RustVariant {
                name: v.name.clone(),
                fields: v
                    .fields
                    .iter()
                    .map(|f| {
                        let toks: Vec<String> = f.split_whitespace().map(String::from).collect();
                        RustType::Raw(super::map_type_tokens(&toks))
                    })
                    .collect(),
                attrs: vec![],
            })
            .collect(),
        ..RustEnum::default()
    };
    items.push(RustItem::Enum(rust_enum));

    // Display impl (only for non-generic enums)
    if tps.is_empty() {
        let arms: Vec<String> = e
            .variants
            .iter()
            .map(|v| {
                if v.fields.is_empty() {
                    format!("{}::{} => write!(f, \"{}\")", e.name, v.name, v.name)
                } else {
                    let underscores: Vec<&str> = (0..v.fields.len()).map(|_| "_").collect();
                    format!(
                        "{}::{}({}) => write!(f, \"{}(...)\")",
                        e.name,
                        v.name,
                        underscores.join(", "),
                        v.name
                    )
                }
            })
            .collect();

        let match_body = format!("match self {{ {} }}", arms.join(", "));
        items.push(RustItem::Impl(RustImpl {
            trait_name: Some("std::fmt::Display".into()),
            target: e.name.clone(),
            type_params: vec![],
            methods: vec![RustFn {
                name: "fmt".into(),
                params: vec![
                    RustParam {
                        name: "&self".into(),
                        ty: RustType::Raw("&Self".into()),
                    },
                    RustParam {
                        name: "f".into(),
                        ty: RustType::Raw("&mut std::fmt::Formatter<'_>".into()),
                    },
                ],
                ret: Some(RustType::Raw("std::fmt::Result".into())),
                body: vec![RustStmt::Raw(match_body)],
                is_pub: false,
                ..RustFn::default()
            }],
        }));
    }

    // Exhaustiveness check (only for non-generic, non-empty enums)
    if !e.variants.is_empty() && tps.is_empty() {
        let arms: Vec<String> = e
            .variants
            .iter()
            .map(|v| {
                if v.fields.is_empty() {
                    format!("{}::{} => \"{}\"", e.name, v.name, v.name)
                } else {
                    let underscores: Vec<&str> = (0..v.fields.len()).map(|_| "_").collect();
                    format!(
                        "{}::{}({}) => \"{}\"",
                        e.name,
                        v.name,
                        underscores.join(", "),
                        v.name
                    )
                }
            })
            .collect();

        let match_body = format!("match v {{ {} }}", arms.join(", "));
        items.push(RustItem::Fn(RustFn {
            name: format!("__exhaustive_check_{}", e.name.to_lowercase()),
            params: vec![RustParam {
                name: "v".into(),
                ty: RustType::Ref(Box::new(RustType::Named(e.name.clone()))),
            }],
            ret: Some(RustType::Raw("&'static str".into())),
            body: vec![RustStmt::Raw(match_body)],
            is_pub: false,
            attrs: vec!["allow(dead_code)".into()],
            doc: vec![
                format!("Compile-time exhaustiveness check for `{}`.", e.name),
                "Adding a variant without updating all match sites causes a build error.".into(),
            ],
            ..RustFn::default()
        }));
    }

    items
}

/// Render a single HIR item to a code string (without prettyplease formatting).
///
/// Used by migration code that appends HIR items to an existing string buffer.
pub fn render_item_raw(item: &RustItem) -> String {
    let mut out = String::new();
    render_item(item, &mut out);
    out
}

/// Like `render_item_raw` but passes `RenderOpts` to control assertion style.
pub(crate) fn render_item_raw_with_opts(item: &RustItem, opts: &RenderOpts) -> String {
    let mut out = String::new();
    render_item_with_opts(item, &mut out, opts);
    out
}

fn render_item_with_opts(item: &RustItem, out: &mut String, opts: &RenderOpts) {
    match item {
        RustItem::Fn(f) => render_fn_opts(f, out, opts),
        // Only functions contain assertions; other items delegate to the
        // regular renderer.
        other => render_item(other, out),
    }
}

// ---------------------------------------------------------------------------
// Builders: AST -> HIR
// ---------------------------------------------------------------------------

/// Build HIR items for an Assura `TypeDef`.
///
/// Returns a `Vec<RustItem>` because some type bodies produce multiple items
/// (e.g., a refined type produces a newtype struct).
pub fn build_type_def(t: &assura_ast::TypeDef) -> Vec<RustItem> {
    use assura_ast::TypeBody;
    let tps = t.type_params.clone();

    match &t.body {
        TypeBody::Struct(fields) => {
            // Always emit `pub` fields: contract modules live in child `mod`s
            // and must access struct fields (e.g. IR `field` → `slot.y`).
            // Private Assura fields would be unreadable outside the crate root.
            let rust_fields: Vec<RustField> = fields
                .iter()
                .map(|f| {
                    let ty_tokens = f.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
                    RustField {
                        name: f.name.clone(),
                        ty: RustType::Raw(crate::map_type_tokens(&ty_tokens)),
                        is_pub: true,
                    }
                })
                .collect();
            // Emit a cfg(test) Arbitrary impl so proptest can invent values for
            // user structs (fields are all pub primitive-mapped types).
            let mut items = vec![RustItem::Struct(RustStruct {
                name: t.name.clone(),
                type_params: tps.clone(),
                fields: rust_fields.clone(),
                derives: vec!["Debug".into(), "Clone".into(), "PartialEq".into()],
                ..RustStruct::default()
            })];
            if tps.is_empty() && !rust_fields.is_empty() {
                let field_names: Vec<&str> = rust_fields.iter().map(|f| f.name.as_str()).collect();
                let field_tys: Vec<String> = rust_fields
                    .iter()
                    .map(|f| match &f.ty {
                        RustType::Raw(s) => s.clone(),
                        other => format!("{other:?}"),
                    })
                    .collect();
                const PRIMS: &[&str] = &[
                    "i64", "u64", "i32", "u32", "bool", "f64", "f32", "i8", "u8", "i16", "u16",
                    "isize", "usize", "String",
                ];
                // Primitives or peer user structs (already emit Arbitrary when
                // declared earlier in the file, e.g. Outer { inner: Inner }).
                let arb_field =
                    |ty: &str| PRIMS.contains(&ty) || crate::types_gen::is_user_type_name(ty);
                if field_tys.iter().all(|ty| arb_field(ty.as_str())) {
                    // Use the same strategies as contract proptest (i32-range for i64)
                    // so field-bearing structs do not re-introduce full-range overflow.
                    let field_strats: Vec<String> = field_tys
                        .iter()
                        .map(|ty| crate::contract::proptest_strategy_for_type(ty))
                        .collect();
                    let destructure = field_names.join(", ");
                    let construct = field_names.join(", ");
                    let strategy = if field_names.len() == 1 {
                        format!(
                            "({}).prop_map(|{destructure}| {} {{ {construct} }})",
                            field_strats[0], t.name
                        )
                    } else {
                        format!(
                            "({}).prop_map(|({destructure})| {} {{ {construct} }})",
                            field_strats.join(", "),
                            t.name
                        )
                    };
                    items.push(RustItem::Raw(format!(
                        "#[cfg(test)]\n\
                         impl proptest::prelude::Arbitrary for {} {{\n\
                             type Parameters = ();\n\
                             type Strategy = proptest::strategy::BoxedStrategy<Self>;\n\
                             fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {{\n\
                                 use proptest::prelude::*;\n\
                                 {strategy}.boxed()\n\
                             }}\n\
                         }}\n",
                        t.name
                    )));
                }
            }
            items
        }
        TypeBody::Alias(tokens) => {
            let rust_ty = crate::map_type_tokens(tokens);
            vec![RustItem::Raw(format!("pub type {} = {rust_ty};\n", t.name))]
        }
        TypeBody::Refined(tokens) => {
            let base = crate::extract_base_type_from_refined(tokens);
            let tps_str = if tps.is_empty() {
                String::new()
            } else {
                format!("<{}>", tps.join(", "))
            };
            vec![RustItem::Raw(format!(
                "#[derive(Debug, Clone, PartialEq)]\npub struct {}{tps_str}(pub {base});\n",
                t.name
            ))]
        }
        TypeBody::Empty => {
            let tps_str = if tps.is_empty() {
                String::new()
            } else {
                format!("<{}>", tps.join(", "))
            };
            if tps.is_empty() {
                vec![RustItem::Raw(format!(
                    "#[derive(Debug, Clone, PartialEq)]\npub struct {}{};\n",
                    t.name, tps_str
                ))]
            } else {
                let phantoms: Vec<String> = tps
                    .iter()
                    .map(|p| format!("std::marker::PhantomData<{p}>"))
                    .collect();
                vec![RustItem::Raw(format!(
                    "#[derive(Debug, Clone, PartialEq)]\npub struct {}{tps_str}({});\n",
                    t.name,
                    phantoms.join(", ")
                ))]
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_simple_fn() {
        let f = RustFn {
            name: "add".into(),
            params: vec![
                RustParam {
                    name: "a".into(),
                    ty: RustType::i64(),
                },
                RustParam {
                    name: "b".into(),
                    ty: RustType::i64(),
                },
            ],
            ret: Some(RustType::i64()),
            body: vec![RustStmt::Expr(RustExpr::Raw("a + b".into()))],
            ..RustFn::default()
        };
        let code = render_items(&[RustItem::Fn(f)]);
        assert!(code.contains("pub fn add(a: i64, b: i64) -> i64"));
        assert!(code.contains("a + b"));
    }

    #[test]
    fn render_unit_struct() {
        let s = RustStruct {
            name: "Marker".into(),
            ..RustStruct::default()
        };
        let code = render_items(&[RustItem::Struct(s)]);
        assert!(code.contains("#[derive(Debug, Clone, PartialEq)]"));
        assert!(code.contains("pub struct Marker;"));
    }

    #[test]
    fn render_struct_with_fields() {
        let s = RustStruct {
            name: "Point".into(),
            fields: vec![
                RustField {
                    name: "x".into(),
                    ty: RustType::f64(),
                    is_pub: true,
                },
                RustField {
                    name: "y".into(),
                    ty: RustType::f64(),
                    is_pub: true,
                },
            ],
            ..RustStruct::default()
        };
        let code = render_items(&[RustItem::Struct(s)]);
        assert!(code.contains("pub struct Point"));
        assert!(code.contains("pub x: f64"));
        assert!(code.contains("pub y: f64"));
    }

    #[test]
    fn render_error_enum() {
        let e = RustEnum {
            name: "SafeDivisionError".into(),
            variants: vec![
                RustVariant {
                    name: "DivByZero".into(),
                    fields: vec![],
                    attrs: vec!["error(\"DivByZero\")".into()],
                },
                RustVariant {
                    name: "Overflow".into(),
                    fields: vec![],
                    attrs: vec!["error(\"Overflow\")".into()],
                },
            ],
            derives: vec!["Debug".into(), "thiserror::Error".into()],
            ..RustEnum::default()
        };
        let code = render_items(&[RustItem::Enum(e)]);
        assert!(code.contains("#[derive(Debug, thiserror::Error)]"));
        assert!(code.contains("pub enum SafeDivisionError"));
        assert!(code.contains("#[error(\"DivByZero\")]"));
        assert!(code.contains("DivByZero,"));
        assert!(code.contains("#[error(\"Overflow\")]"));
        assert!(code.contains("Overflow,"));
    }

    #[test]
    fn render_fn_with_assert() {
        let f = RustFn {
            name: "check".into(),
            params: vec![RustParam {
                name: "x".into(),
                ty: RustType::i64(),
            }],
            ret: Some(RustType::i64()),
            body: vec![
                RustStmt::Assert {
                    cond: "(x > 0)".into(),
                    label: "requires".into(),
                },
                RustStmt::Let {
                    name: "__assura_result".into(),
                    ty: Some(RustType::i64()),
                    init: RustExpr::Todo("implementation provided by AI agent".into()),
                },
                RustStmt::Expr(RustExpr::Ident("__assura_result".into())),
            ],
            ..RustFn::default()
        };
        let code = render_items(&[RustItem::Fn(f)]);
        assert!(code.contains("debug_assert!"));
        assert!(code.contains("requires"));
        assert!(code.contains("todo!"));
    }

    #[test]
    fn render_const() {
        let c = RustConst {
            name: "MAX_SIZE".into(),
            ty: RustType::u64(),
            value: "1024".into(),
            is_pub: true,
            doc: vec![],
        };
        let code = render_items(&[RustItem::Const(c)]);
        assert!(code.contains("pub const MAX_SIZE: u64 = 1024;"));
    }

    #[test]
    fn render_generic_type() {
        let ty = RustType::result(RustType::i64(), RustType::Named("MyError".into()));
        assert_eq!(render_type(&ty), "Result<i64, MyError>");
    }

    #[test]
    fn render_module() {
        let m = RustMod {
            name: "contract_foo".into(),
            items: vec![RustItem::Comment("inner".into())],
            is_pub: true,
            doc: vec!["Contract: Foo".into()],
        };
        let code = render_items(&[RustItem::Mod(m)]);
        assert!(code.contains("/// Contract: Foo"));
        assert!(code.contains("pub mod contract_foo"));
    }

    #[test]
    fn render_impl_block() {
        let i = RustImpl {
            trait_name: Some("Display".into()),
            target: "Color".into(),
            type_params: vec![],
            methods: vec![RustFn {
                name: "fmt".into(),
                params: vec![
                    RustParam {
                        name: "&self".into(),
                        ty: RustType::Raw("&Self".into()),
                    },
                    RustParam {
                        name: "f".into(),
                        ty: RustType::Raw("&mut std::fmt::Formatter<'_>".into()),
                    },
                ],
                ret: Some(RustType::Raw("std::fmt::Result".into())),
                body: vec![RustStmt::Raw(
                    "match self { _ => write!(f, \"Color\") }".into(),
                )],
                is_pub: false,
                ..RustFn::default()
            }],
        };
        let code = render_items(&[RustItem::Impl(i)]);
        assert!(code.contains("impl Display for Color"));
    }

    #[test]
    fn type_shorthand_helpers() {
        assert_eq!(render_type(&RustType::i64()), "i64");
        assert_eq!(render_type(&RustType::u64()), "u64");
        assert_eq!(render_type(&RustType::f64()), "f64");
        assert_eq!(render_type(&RustType::bool()), "bool");
        assert_eq!(render_type(&RustType::string()), "String");
        assert_eq!(render_type(&RustType::bytes()), "Vec<u8>");
        assert_eq!(render_type(&RustType::Unit), "()");
        assert_eq!(render_type(&RustType::Never), "!");
    }

    #[test]
    fn render_trait_def() {
        let t = RustTrait {
            name: "Serializable".into(),
            type_params: vec![],
            supertraits: vec!["Clone".into()],
            methods: vec![RustFn {
                name: "serialize".into(),
                params: vec![RustParam {
                    name: "&self".into(),
                    ty: RustType::Raw("&Self".into()),
                }],
                ret: Some(RustType::bytes()),
                body: vec![],
                is_pub: false,
                ..RustFn::default()
            }],
            is_pub: true,
            doc: vec![],
        };
        let code = render_items(&[RustItem::Trait(t)]);
        assert!(code.contains("pub trait Serializable: Clone"));
    }

    // --- Structural tests ---

    #[test]
    fn count_asserts_in_fn() {
        let f = RustFn {
            name: "check".into(),
            params: vec![RustParam {
                name: "x".into(),
                ty: RustType::i64(),
            }],
            ret: Some(RustType::i64()),
            body: vec![
                RustStmt::Assert {
                    cond: "(x > 0)".into(),
                    label: "requires".into(),
                },
                RustStmt::Assert {
                    cond: "(x < 100)".into(),
                    label: "requires".into(),
                },
                RustStmt::Expr(RustExpr::Ident("x".into())),
            ],
            ..RustFn::default()
        };
        let assert_count = f
            .body
            .iter()
            .filter(|s| matches!(s, RustStmt::Assert { .. }))
            .count();
        assert_eq!(assert_count, 2, "should have exactly 2 assert statements");
    }

    // --- HIR builder structural tests ---

    #[test]
    fn build_error_enum_structure() {
        let item = build_error_enum("SafeDivision", &["DivByZero".into(), "Overflow".into()]);
        if let RustItem::Enum(e) = &item {
            assert_eq!(e.name, "SafeDivisionError");
            assert_eq!(e.variants.len(), 2);
            assert_eq!(e.variants[0].name, "DivByZero");
            assert_eq!(e.variants[1].name, "Overflow");
            assert!(e.derives.contains(&"thiserror::Error".to_string()));
            // Each variant has an #[error("...")] attribute
            for v in &e.variants {
                assert_eq!(v.attrs.len(), 1);
                assert!(v.attrs[0].starts_with("error(\""));
            }
        } else {
            panic!("Expected RustItem::Enum");
        }
    }

    #[test]
    fn build_enum_def_structure() {
        let enum_def = assura_ast::EnumDef {
            name: "Color".into(),
            type_params: vec![],
            variants: vec![
                assura_ast::EnumVariant {
                    name: "Red".into(),
                    fields: vec![],
                },
                assura_ast::EnumVariant {
                    name: "Blue".into(),
                    fields: vec![],
                },
                assura_ast::EnumVariant {
                    name: "Custom".into(),
                    fields: vec!["Int".into()],
                },
            ],
        };
        let items = build_enum_def(&enum_def);
        // Should produce: enum, Display impl, exhaustiveness check
        assert_eq!(items.len(), 3, "enum + Display + exhaustive");

        // First item: the enum
        if let RustItem::Enum(e) = &items[0] {
            assert_eq!(e.name, "Color");
            assert_eq!(e.variants.len(), 3);
            assert_eq!(e.variants[2].name, "Custom");
            assert_eq!(e.variants[2].fields.len(), 1);
        } else {
            panic!("Expected RustItem::Enum");
        }

        // Second item: Display impl
        assert!(
            matches!(&items[1], RustItem::Impl(i) if i.trait_name.as_deref() == Some("std::fmt::Display"))
        );

        // Third item: exhaustiveness check fn
        assert!(matches!(&items[2], RustItem::Fn(f) if f.name == "__exhaustive_check_color"));
    }

    #[test]
    fn build_enum_def_generic_no_display() {
        let enum_def = assura_ast::EnumDef {
            name: "Option".into(),
            type_params: vec!["T".into()],
            variants: vec![
                assura_ast::EnumVariant {
                    name: "Some".into(),
                    fields: vec!["T".into()],
                },
                assura_ast::EnumVariant {
                    name: "None".into(),
                    fields: vec![],
                },
            ],
        };
        let items = build_enum_def(&enum_def);
        // Generic enums: no Display impl, no exhaustiveness check
        assert_eq!(
            items.len(),
            1,
            "only enum, no Display or exhaustive for generic"
        );
        assert!(matches!(&items[0], RustItem::Enum(e) if e.type_params == vec!["T"]));
    }

    // --- Round-trip test ---

    #[test]
    fn round_trip_parse_format() {
        // Build HIR -> render -> parse via syn -> re-format via prettyplease
        let items = vec![
            RustItem::Fn(RustFn {
                name: "add".into(),
                params: vec![
                    RustParam {
                        name: "a".into(),
                        ty: RustType::i64(),
                    },
                    RustParam {
                        name: "b".into(),
                        ty: RustType::i64(),
                    },
                ],
                ret: Some(RustType::i64()),
                body: vec![RustStmt::Expr(RustExpr::Raw("a + b".into()))],
                ..RustFn::default()
            }),
            RustItem::Struct(RustStruct {
                name: "Point".into(),
                fields: vec![
                    RustField {
                        name: "x".into(),
                        ty: RustType::f64(),
                        is_pub: true,
                    },
                    RustField {
                        name: "y".into(),
                        ty: RustType::f64(),
                        is_pub: true,
                    },
                ],
                ..RustStruct::default()
            }),
        ];

        let rendered = render_items(&items);
        // Parse the rendered output back with syn
        let parsed = syn::parse_file(&rendered);
        assert!(
            parsed.is_ok(),
            "Rendered HIR should parse as valid Rust: {:?}",
            parsed.err()
        );

        // Re-format the parsed result
        let reparsed = prettyplease::unparse(&parsed.unwrap());
        // The double-formatted result should be identical (idempotent)
        assert_eq!(rendered, reparsed, "Formatting should be idempotent");
    }

    // --- Type mapping coverage test ---

    #[test]
    fn type_rendering_coverage() {
        // Test all RustType variants render correctly
        let cases = vec![
            (RustType::Named("MyStruct".into()), "MyStruct"),
            (
                RustType::Generic("Vec".into(), vec![RustType::Named("u8".into())]),
                "Vec<u8>",
            ),
            (
                RustType::Ref(Box::new(RustType::Named("str".into()))),
                "&str",
            ),
            (RustType::RefMut(Box::new(RustType::i64())), "&mut i64"),
            (
                RustType::Tuple(vec![RustType::i64(), RustType::bool()]),
                "(i64, bool)",
            ),
            (RustType::Unit, "()"),
            (RustType::Never, "!"),
            (RustType::Raw("Box<dyn Fn()>".into()), "Box<dyn Fn()>"),
        ];
        for (ty, expected) in cases {
            assert_eq!(
                render_type(&ty),
                expected,
                "type rendering mismatch for {:?}",
                ty
            );
        }
    }

    // --- Malformed HIR rejection test ---

    #[test]
    fn malformed_hir_produces_warning() {
        // A function with syntax errors in its body should produce a warning comment
        let items = vec![RustItem::Fn(RustFn {
            name: "bad".into(),
            body: vec![RustStmt::Raw("this is {{ not valid {{ rust".into())],
            ..RustFn::default()
        })];
        let rendered = render_items(&items);
        assert!(
            rendered.contains("WARNING"),
            "Invalid HIR should produce a warning in output"
        );
    }
}
