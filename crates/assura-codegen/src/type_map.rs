//! Reverse type mapping: Rust types to Assura types.
//!
//! The forward mapping (Assura -> Rust) lives in `map_type_token()` in `lib.rs`.
//! This module provides the inverse: given a Rust type string, produce the
//! canonical Assura type. Used by AI contract generation templates and the
//! `assura infer` command.

/// Map a Rust type string to its Assura equivalent.
///
/// Handles primitives, standard library generics, references, and nested types.
/// Unknown types are passed through unchanged (they become user-defined types
/// in Assura).
///
/// # Examples
/// ```
/// use assura_codegen::type_map::rust_type_to_assura;
/// assert_eq!(rust_type_to_assura("i64"), "Int");
/// assert_eq!(rust_type_to_assura("Vec<u8>"), "Bytes");
/// assert_eq!(rust_type_to_assura("Vec<i64>"), "List<Int>");
/// assert_eq!(rust_type_to_assura("Option<i64>"), "Int?");
/// ```
pub fn rust_type_to_assura(rust_type: &str) -> String {
    let trimmed = rust_type.trim();

    // Handle references: &str, &[u8], &T, &mut T
    if let Some(inner) = trimmed.strip_prefix('&') {
        let inner = inner.trim_start();
        if let Some(inner) = inner.strip_prefix("mut ") {
            return rust_type_to_assura(inner.trim());
        }
        if inner == "str" {
            return "String".to_string();
        }
        if inner == "[u8]" {
            return "Bytes".to_string();
        }
        // &[T] -> List<T>
        if let Some(slice_inner) = inner.strip_prefix('[')
            && let Some(slice_inner) = slice_inner.strip_suffix(']')
        {
            let mapped = rust_type_to_assura(slice_inner.trim());
            return format!("List<{mapped}>");
        }
        return rust_type_to_assura(inner);
    }

    // Handle tuple types: (T, U, ...) -> (T, U, ...)
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        if inner.is_empty() {
            return "Unit".to_string();
        }
        let parts = split_type_args(inner);
        let mapped: Vec<String> = parts.iter().map(|p| rust_type_to_assura(p)).collect();
        return format!("({})", mapped.join(", "));
    }

    // Handle simple primitives first (no generics)
    if !trimmed.contains('<') {
        return map_simple_rust_type(trimmed).to_string();
    }

    // Handle generic types: Name<Args>
    if let Some((base, args)) = split_generic(trimmed) {
        let base_trimmed = base.trim();
        let type_args = split_type_args(args);

        match base_trimmed {
            // Vec<u8> -> Bytes, Vec<T> -> List<T>
            "Vec" => {
                if type_args.len() == 1 && type_args[0].trim() == "u8" {
                    "Bytes".to_string()
                } else if type_args.len() == 1 {
                    let inner = rust_type_to_assura(type_args[0].trim());
                    format!("List<{inner}>")
                } else {
                    format!("Vec<{}>", map_type_arg_list(&type_args))
                }
            }

            // Option<T> -> T?
            "Option" => {
                if type_args.len() == 1 {
                    let inner = rust_type_to_assura(type_args[0].trim());
                    format!("{inner}?")
                } else {
                    format!("Option<{}>", map_type_arg_list(&type_args))
                }
            }

            // Result<T, E> -> Result<T, E> (context-dependent, pass through)
            "Result" => {
                format!("Result<{}>", map_type_arg_list(&type_args))
            }

            // Map types
            "HashMap" | "BTreeMap" | "std::collections::HashMap" | "std::collections::BTreeMap" => {
                format!("Map<{}>", map_type_arg_list(&type_args))
            }

            // Set types
            "HashSet" | "BTreeSet" | "std::collections::HashSet" | "std::collections::BTreeSet" => {
                if type_args.len() == 1 {
                    let inner = rust_type_to_assura(type_args[0].trim());
                    format!("Set<{inner}>")
                } else {
                    format!("Set<{}>", map_type_arg_list(&type_args))
                }
            }

            // Box<T>, Arc<T>, Rc<T>, Cow<T> -> just T (wrapper erasure)
            "Box" | "Arc" | "Rc" | "Cow" | "std::sync::Arc" | "std::rc::Rc"
            | "std::borrow::Cow" => {
                if type_args.len() == 1 {
                    rust_type_to_assura(type_args[0].trim())
                } else {
                    format!("{base_trimmed}<{}>", map_type_arg_list(&type_args))
                }
            }

            // Unknown generic: pass through with mapped args
            _ => {
                let mapped_base = map_simple_rust_type(base_trimmed);
                format!("{mapped_base}<{}>", map_type_arg_list(&type_args))
            }
        }
    } else {
        map_simple_rust_type(trimmed).to_string()
    }
}

/// Map a simple (non-generic) Rust type to Assura.
fn map_simple_rust_type(ty: &str) -> &str {
    // Strip path prefixes so `std::num::NonZeroU8` matches.
    let ty = ty.rsplit("::").next().unwrap_or(ty).trim();
    match ty {
        // Signed integers -> Int
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => "Int",
        // Unsigned integers -> Nat
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => "Nat",
        // NonZero* (std::num) — positive integer wrappers
        "NonZeroU8" | "NonZeroU16" | "NonZeroU32" | "NonZeroU64" | "NonZeroU128"
        | "NonZeroUsize" => "Nat",
        "NonZeroI8" | "NonZeroI16" | "NonZeroI32" | "NonZeroI64" | "NonZeroI128"
        | "NonZeroIsize" => "Int",
        // Floats
        "f32" | "f64" => "Float",
        // Bool
        "bool" => "Bool",
        // String types
        "String" | "str" => "String",
        // Unit
        "()" => "Unit",
        // Never
        "!" | "Infallible" | "std::convert::Infallible" => "Never",
        // Bytes as a standalone type name
        "Bytes" | "bytes::Bytes" => "Bytes",
        // Pass through anything else
        _ => ty,
    }
}

/// Split `Name<A, B, C>` into `("Name", "A, B, C")`.
fn split_generic(ty: &str) -> Option<(&str, &str)> {
    let open = ty.find('<')?;
    let close = ty.rfind('>')?;
    if close <= open {
        return None;
    }
    Some((&ty[..open], &ty[open + 1..close]))
}

/// Split a comma-separated type argument list, respecting nested `<>`.
fn split_type_args(args: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut paren_depth = 0i32;
    let mut start = 0;

    for (i, ch) in args.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            ',' if depth == 0 && paren_depth == 0 => {
                result.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < args.len() {
        result.push(&args[start..]);
    }
    result
}

/// Map a list of type arguments recursively.
fn map_type_arg_list(args: &[&str]) -> String {
    args.iter()
        .map(|a| rust_type_to_assura(a.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Like [`rust_type_to_assura`], but maps unrecognized Rust types to `Unknown`
/// instead of passing them through. Also handles `impl Trait`, `dyn Trait`,
/// `Self`, and lifetime annotations that the standard mapper does not.
///
/// Used by `assura audit` where unknown Rust types would cause A02001
/// resolution errors that prevent verification from running.
pub fn rust_type_to_assura_lenient(rust_type: &str) -> String {
    let ty = rust_type.trim();
    // Unmappable Rust type syntax: return Unknown directly
    if ty.starts_with("impl ") || ty.starts_with("dyn ") || ty == "Self" {
        return "Unknown".to_string();
    }
    if ty.starts_with("Pin<") || ty.starts_with("PhantomData") {
        return "Unknown".to_string();
    }
    // Strip lifetime from references: &'a T -> &T, &'_ T -> &T
    let cleaned = if ty.starts_with("&'") {
        if let Some(space_idx) = ty[1..].find(' ') {
            format!("&{}", &ty[2 + space_idx..])
        } else {
            ty.to_string()
        }
    } else {
        ty.to_string()
    };
    let mapped = rust_type_to_assura(&cleaned);
    sanitize_mapped_type(&mapped)
}

/// Recursively replace type names that are not valid Assura types with `Unknown`.
fn sanitize_mapped_type(ty: &str) -> String {
    let ty = ty.trim();
    if ty.is_empty() {
        return "Unknown".to_string();
    }
    // Optional: T?
    if let Some(inner) = ty.strip_suffix('?') {
        let s = sanitize_mapped_type(inner);
        return format!("{s}?");
    }
    // Tuple: (T, U, ...)
    if ty.starts_with('(') && ty.ends_with(')') {
        let inner = &ty[1..ty.len() - 1];
        if inner.is_empty() {
            return "Unit".to_string();
        }
        let parts = split_type_args(inner);
        let sanitized: Vec<String> = parts
            .iter()
            .map(|p| sanitize_mapped_type(p.trim()))
            .collect();
        return format!("({})", sanitized.join(", "));
    }
    // Known generic bases: List<...>, Map<...>, Set<...>, Result<...>
    if let Some((base, args)) = split_generic(ty) {
        let base = base.trim();
        match base {
            "List" | "Map" | "Set" | "Result" => {
                let parts = split_type_args(args);
                let sanitized: Vec<String> = parts
                    .iter()
                    .map(|p| sanitize_mapped_type(p.trim()))
                    .collect();
                return format!("{base}<{}>", sanitized.join(", "));
            }
            _ => return "Unknown".to_string(),
        }
    }
    // Known Assura primitive types
    match ty {
        "Int" | "Nat" | "Float" | "Bool" | "String" | "Bytes" | "Unit" | "Never" | "Unknown" => {
            ty.to_string()
        }
        _ => "Unknown".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Primitive mapping ---

    #[test]
    fn signed_integers_map_to_int() {
        for ty in &["i8", "i16", "i32", "i64", "i128", "isize"] {
            assert_eq!(rust_type_to_assura(ty), "Int", "failed for {ty}");
        }
    }

    #[test]
    fn unsigned_integers_map_to_nat() {
        for ty in &["u8", "u16", "u32", "u64", "u128", "usize"] {
            assert_eq!(rust_type_to_assura(ty), "Nat", "failed for {ty}");
        }
    }

    #[test]
    fn nonzero_integers_map_to_int_or_nat() {
        for ty in &[
            "NonZeroU8",
            "NonZeroU16",
            "NonZeroU32",
            "NonZeroU64",
            "NonZeroUsize",
        ] {
            assert_eq!(rust_type_to_assura(ty), "Nat", "failed for {ty}");
        }
        for ty in &[
            "NonZeroI8",
            "NonZeroI16",
            "NonZeroI32",
            "NonZeroI64",
            "NonZeroIsize",
        ] {
            assert_eq!(rust_type_to_assura(ty), "Int", "failed for {ty}");
        }
        assert_eq!(rust_type_to_assura("std::num::NonZeroU8"), "Nat");
    }

    #[test]
    fn floats_map_to_float() {
        assert_eq!(rust_type_to_assura("f32"), "Float");
        assert_eq!(rust_type_to_assura("f64"), "Float");
    }

    #[test]
    fn bool_maps_to_bool() {
        assert_eq!(rust_type_to_assura("bool"), "Bool");
    }

    #[test]
    fn string_types() {
        assert_eq!(rust_type_to_assura("String"), "String");
        assert_eq!(rust_type_to_assura("&str"), "String");
    }

    #[test]
    fn unit_and_never() {
        assert_eq!(rust_type_to_assura("()"), "Unit");
        assert_eq!(rust_type_to_assura("!"), "Never");
        assert_eq!(rust_type_to_assura("Infallible"), "Never");
    }

    // --- Collection mapping ---

    #[test]
    fn vec_u8_maps_to_bytes() {
        assert_eq!(rust_type_to_assura("Vec<u8>"), "Bytes");
    }

    #[test]
    fn vec_maps_to_list() {
        assert_eq!(rust_type_to_assura("Vec<i64>"), "List<Int>");
        assert_eq!(rust_type_to_assura("Vec<String>"), "List<String>");
    }

    #[test]
    fn map_types() {
        assert_eq!(
            rust_type_to_assura("HashMap<String, i64>"),
            "Map<String, Int>"
        );
        assert_eq!(
            rust_type_to_assura("BTreeMap<String, u64>"),
            "Map<String, Nat>"
        );
    }

    #[test]
    fn set_types() {
        assert_eq!(rust_type_to_assura("HashSet<i64>"), "Set<Int>");
        assert_eq!(rust_type_to_assura("BTreeSet<String>"), "Set<String>");
    }

    // --- Option mapping ---

    #[test]
    fn option_maps_to_nullable() {
        assert_eq!(rust_type_to_assura("Option<i64>"), "Int?");
        assert_eq!(rust_type_to_assura("Option<String>"), "String?");
    }

    // --- Reference erasure ---

    #[test]
    fn references_are_erased() {
        assert_eq!(rust_type_to_assura("&i64"), "Int");
        assert_eq!(rust_type_to_assura("&mut i64"), "Int");
        assert_eq!(rust_type_to_assura("&[u8]"), "Bytes");
        assert_eq!(rust_type_to_assura("&[i64]"), "List<Int>");
    }

    // --- Wrapper erasure ---

    #[test]
    fn smart_pointers_are_erased() {
        assert_eq!(rust_type_to_assura("Box<i64>"), "Int");
        assert_eq!(rust_type_to_assura("Arc<String>"), "String");
        assert_eq!(rust_type_to_assura("Rc<Vec<i64>>"), "List<Int>");
    }

    // --- Nested generics ---

    #[test]
    fn nested_generics() {
        assert_eq!(rust_type_to_assura("Vec<Option<i64>>"), "List<Int?>");
        assert_eq!(
            rust_type_to_assura("Vec<Option<BTreeMap<String, i64>>>"),
            "List<Map<String, Int>?>"
        );
    }

    // --- Tuples ---

    #[test]
    fn tuple_types() {
        assert_eq!(rust_type_to_assura("(i64, u64)"), "(Int, Nat)");
        assert_eq!(
            rust_type_to_assura("(String, bool, f64)"),
            "(String, Bool, Float)"
        );
    }

    // --- Unknown passthrough ---

    #[test]
    fn unknown_types_pass_through() {
        assert_eq!(rust_type_to_assura("MyCustomStruct"), "MyCustomStruct");
        assert_eq!(rust_type_to_assura("MyGeneric<i64>"), "MyGeneric<Int>");
    }

    // --- Result passthrough ---

    #[test]
    fn result_type() {
        assert_eq!(
            rust_type_to_assura("Result<i64, String>"),
            "Result<Int, String>"
        );
    }

    // --- Lenient mapping (for audit) ---

    #[test]
    fn lenient_primitives_pass_through() {
        assert_eq!(rust_type_to_assura_lenient("i64"), "Int");
        assert_eq!(rust_type_to_assura_lenient("u32"), "Nat");
        assert_eq!(rust_type_to_assura_lenient("bool"), "Bool");
        assert_eq!(rust_type_to_assura_lenient("String"), "String");
        assert_eq!(rust_type_to_assura_lenient("f64"), "Float");
    }

    #[test]
    fn lenient_unknown_types_become_unknown() {
        assert_eq!(rust_type_to_assura_lenient("Config"), "Unknown");
        assert_eq!(rust_type_to_assura_lenient("PathBuf"), "Unknown");
        assert_eq!(rust_type_to_assura_lenient("MyStruct"), "Unknown");
    }

    #[test]
    fn lenient_unknown_generics_become_unknown() {
        assert_eq!(rust_type_to_assura_lenient("MyGeneric<i64>"), "Unknown");
        assert_eq!(rust_type_to_assura_lenient("Foo<Bar, Baz>"), "Unknown");
    }

    #[test]
    fn lenient_known_generics_preserve_structure() {
        assert_eq!(rust_type_to_assura_lenient("Vec<i64>"), "List<Int>");
        assert_eq!(
            rust_type_to_assura_lenient("HashMap<String, i64>"),
            "Map<String, Int>"
        );
        assert_eq!(rust_type_to_assura_lenient("Option<bool>"), "Bool?");
    }

    #[test]
    fn lenient_nested_unknown_in_known_generic() {
        assert_eq!(rust_type_to_assura_lenient("Vec<Config>"), "List<Unknown>");
        assert_eq!(
            rust_type_to_assura_lenient("Result<i64, MyError>"),
            "Result<Int, Unknown>"
        );
    }

    #[test]
    fn lenient_impl_dyn_self() {
        assert_eq!(rust_type_to_assura_lenient("impl Iterator"), "Unknown");
        assert_eq!(rust_type_to_assura_lenient("dyn Trait"), "Unknown");
        assert_eq!(rust_type_to_assura_lenient("Self"), "Unknown");
    }

    #[test]
    fn lenient_lifetime_references() {
        assert_eq!(rust_type_to_assura_lenient("&'a str"), "String");
        assert_eq!(rust_type_to_assura_lenient("&'_ i64"), "Int");
    }

    #[test]
    fn lenient_pin_phantomdata() {
        assert_eq!(
            rust_type_to_assura_lenient("Pin<Box<dyn Future>>"),
            "Unknown"
        );
        assert_eq!(rust_type_to_assura_lenient("PhantomData<T>"), "Unknown");
    }

    #[test]
    fn lenient_wrapper_erasure_then_sanitize() {
        assert_eq!(rust_type_to_assura_lenient("Box<i64>"), "Int");
        assert_eq!(rust_type_to_assura_lenient("Arc<String>"), "String");
        assert_eq!(rust_type_to_assura_lenient("Arc<Config>"), "Unknown");
    }
}
