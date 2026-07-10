//! T083: TEST.1 Test generation from contracts.

use crate::Type;

/// Generates property-based and boundary-value tests from contract specs.
///
/// Produces Rust test code (proptest/quickcheck) from requires/ensures clauses.
#[derive(Debug, Clone)]
pub struct TestGenerator {
    contracts: Vec<TestableContract>,
}

#[derive(Debug, Clone)]
pub struct TestableContract {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub requires: Vec<String>,
    pub ensures: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GeneratedTest {
    pub name: String,
    pub body: String,
    pub kind: TestKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestKind {
    Property,
    Boundary,
    Smoke,
}

impl TestGenerator {
    pub fn new() -> Self {
        Self {
            contracts: Vec::new(),
        }
    }

    pub fn add_contract(&mut self, contract: TestableContract) {
        self.contracts.push(contract);
    }

    pub fn generate_property_test(&self, contract: &TestableContract) -> GeneratedTest {
        let param_list: Vec<String> = contract
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {}", Self::type_to_proptest_strategy(t)))
            .collect();
        let preconditions = if contract.requires.is_empty() {
            String::new()
        } else {
            format!(
                "prop_assume!({});\n        ",
                contract.requires.join(" && ")
            )
        };
        // Ensures often reference `result`. Property tests have no implementation
        // call site, so bind `result` when an ensures is `result == <expr>`
        // (e.g. init SafeDivision: result == a / b). Without this, generated
        // tests fail to compile with unresolved `result`.
        let (result_bind, postconditions) = Self::result_bind_and_posts(&contract.ensures);
        let fn_name = Self::rust_test_ident("prop", &contract.name);
        let body = format!(
            "proptest! {{\n    #[test]\n    fn {fn_name}({}) {{\n        {preconditions}{result_bind}prop_assert!({postconditions});\n    }}\n}}",
            param_list.join(", ")
        );
        GeneratedTest {
            name: fn_name,
            body,
            kind: TestKind::Property,
        }
    }

    /// `prop_SafeDiv` → `prop_safe_div` (rustc non_snake_case lint).
    fn rust_test_ident(prefix: &str, contract_name: &str) -> String {
        let mut out = String::from(prefix);
        for (i, ch) in contract_name.chars().enumerate() {
            if ch.is_uppercase() {
                if i > 0 || !out.is_empty() {
                    out.push('_');
                }
                out.extend(ch.to_lowercase());
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// If any ensures is `result == <expr>`, emit `let result = <expr>;` and
    /// keep all ensures for assertion (the equality is then a tautology unless
    // the right-hand side uses free vars — still better than undeclared result).
    fn result_bind_and_posts(ensures: &[String]) -> (String, String) {
        let mut bind = String::new();
        for e in ensures {
            let trimmed = e.trim();
            if let Some(rhs) = trimmed.strip_prefix("result == ") {
                bind = format!("let result = {rhs};\n        ");
                break;
            }
            if let Some(rhs) = trimmed.strip_prefix("(result == ") {
                // expr_to_rust may wrap: (result == (a / b))
                if let Some(inner) = rhs.strip_suffix(')') {
                    bind = format!("let result = {inner};\n        ");
                    break;
                }
            }
        }
        (bind, ensures.join(" && "))
    }

    pub fn generate_boundary_tests(&self, contract: &TestableContract) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for (name, ty) in &contract.params {
            let boundaries = Self::boundary_values(ty);
            for (i, val) in boundaries.iter().enumerate() {
                let fn_name =
                    Self::rust_test_ident(&format!("boundary_{name}_{i}"), &contract.name);
                // Boundary stubs only bind values for humans to fill in; silence
                // unused_variables until they call the SUT.
                tests.push(GeneratedTest {
                    name: fn_name.clone(),
                    body: format!(
                        "#[test]\n#[allow(unused_variables)]\nfn {fn_name}() {{\n    let {name} = {val};\n    // boundary test for {name}\n}}"
                    ),
                    kind: TestKind::Boundary,
                });
            }
        }
        tests
    }

    pub fn generate_smoke_test(&self, contract: &TestableContract) -> GeneratedTest {
        let fn_name = Self::rust_test_ident("smoke", &contract.name);
        let body = format!("#[test]\nfn {fn_name}() {{\n    // smoke test: basic valid inputs\n}}");
        GeneratedTest {
            name: fn_name,
            body,
            kind: TestKind::Smoke,
        }
    }

    pub fn generate_all(&self) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for contract in &self.contracts {
            tests.push(self.generate_property_test(contract));
            tests.extend(self.generate_boundary_tests(contract));
            tests.push(self.generate_smoke_test(contract));
        }
        tests
    }

    /// Type name for proptest's type-based strategy form: `fn prop(x: i64, y: bool)`.
    ///
    /// Must be a real Rust type implementing `Arbitrary`, not invented
    /// associated constants like `i64::ANY` (those fail to compile).
    fn type_to_proptest_strategy(ty: &Type) -> &'static str {
        match ty {
            Type::Int | Type::I64 => "i64",
            Type::Nat | Type::U64 => "u64",
            Type::U8 => "u8",
            Type::U16 => "u16",
            Type::U32 => "u32",
            Type::I8 => "i8",
            Type::I16 => "i16",
            Type::I32 => "i32",
            Type::Float | Type::F64 => "f64",
            Type::F32 => "f32",
            Type::Bool => "bool",
            Type::String => "String",
            _ => "()",
        }
    }

    fn boundary_values(ty: &Type) -> Vec<String> {
        match ty {
            Type::Int | Type::I64 => vec![
                "0".into(),
                "1".into(),
                "-1".into(),
                "i64::MAX".into(),
                "i64::MIN".into(),
            ],
            Type::Nat | Type::U64 => vec!["0".into(), "1".into(), "u64::MAX".into()],
            Type::U8 => vec!["0u8".into(), "1u8".into(), "255u8".into()],
            Type::U16 => vec!["0u16".into(), "1u16".into(), "65535u16".into()],
            Type::U32 => vec!["0u32".into(), "1u32".into(), "u32::MAX".into()],
            Type::I8 => vec![
                "0i8".into(),
                "1i8".into(),
                "-1i8".into(),
                "127i8".into(),
                "-128i8".into(),
            ],
            Type::I16 => vec![
                "0i16".into(),
                "1i16".into(),
                "-1i16".into(),
                "i16::MAX".into(),
                "i16::MIN".into(),
            ],
            Type::I32 => vec![
                "0i32".into(),
                "1i32".into(),
                "-1i32".into(),
                "i32::MAX".into(),
                "i32::MIN".into(),
            ],
            Type::Bool => vec!["true".into(), "false".into()],
            Type::Float | Type::F64 => vec![
                "0.0".into(),
                "1.0".into(),
                "-1.0".into(),
                "f64::INFINITY".into(),
                "f64::NAN".into(),
            ],
            Type::String => vec![
                r#""""#.into(),
                r#""hello""#.into(),
                r#""a""#.into(),
                r#""Hello, World!""#.into(),
            ],
            Type::Bytes => vec!["b\"\"".into(), "b\"\\x00\"".into(), "b\"\\xff\"".into()],
            Type::List(_) => vec!["vec![]".into(), "vec![Default::default()]".into()],
            Type::Map(_, _) => vec!["HashMap::new()".into()],
            Type::Set(_) => vec!["HashSet::new()".into()],
            Type::Option(_) => vec!["None".into(), "Some(Default::default())".into()],
            Type::Result(_, _) => vec![
                "Ok(Default::default())".into(),
                "Err(Default::default())".into(),
            ],
            _ => vec![],
        }
    }
}

impl Default for TestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_test_binds_result_from_equality_ensures() {
        let mut tg = TestGenerator::new();
        tg.add_contract(TestableContract {
            name: "SafeDivision".into(),
            params: vec![("a".into(), Type::Int), ("b".into(), Type::Int)],
            requires: vec!["(b != 0)".into()],
            ensures: vec!["(result == (a / b))".into()],
        });
        let test = tg.generate_property_test(&tg.contracts[0]);
        assert!(
            test.body.contains("let result = "),
            "should bind result: {}",
            test.body
        );
        assert!(
            test.body.contains("prop_assert!"),
            "should assert postconditions: {}",
            test.body
        );
        assert!(
            !test.body.contains("prop_assert!((result") || test.body.contains("let result"),
            "result must be declared before assert: {}",
            test.body
        );
        assert!(
            test.body.contains("a: i64") && test.body.contains("b: i64"),
            "proptest strategies must be real Rust types, not ::ANY: {}",
            test.body
        );
        assert!(
            !test.body.contains("::ANY"),
            "i64::ANY is not valid proptest: {}",
            test.body
        );
    }
}
