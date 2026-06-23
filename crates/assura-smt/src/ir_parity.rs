//! Cross-backend IR encoding parity harness (#291).

#[cfg(test)]
mod tests {
    use crate::havoc_assume::HavocAssumeInput;
    use crate::ir::IrFunction;
    use crate::ir_encode::IrEncodeContext;
    use crate::ir_encode::{
        assert_ir_blocks_inlined, assert_ir_blocks_missing_uf_fallback, branch_if_else_ir_fixture,
        branch_if_else_missing_blocks_fixture,
    };

    /// SMT-LIB2 shell path is only compiled when `cvc5-verify` is off
    /// (`cvc5_ir_smtlib` is `cfg(not(feature = "cvc5-verify"))`).
    #[cfg(not(feature = "cvc5-verify"))]
    fn shell_ir_output(func: &IrFunction, enc_ctx: IrEncodeContext<'_>) -> String {
        use std::collections::HashSet;

        use crate::cvc5_backend::cvc5_ir_smtlib::append_ir_body_constraints_smtlib;

        let mut script = String::new();
        let mut vars: HashSet<String> = HashSet::new();
        append_ir_body_constraints_smtlib(&mut script, &mut vars, func, &["x".into()], enc_ctx);
        script
    }

    #[cfg(feature = "z3-verify")]
    fn z3_ir_output(func: &IrFunction, enc_ctx: IrEncodeContext<'_>) -> String {
        use crate::z3_backend::apply_havoc_assume_z3;
        use crate::z3_backend::encoder::Encoder;

        z3::with_z3_config(&z3::Config::new(), || {
            let mut encoder = Encoder::new();
            apply_havoc_assume_z3(
                &mut encoder,
                &HavocAssumeInput {
                    requires: &[],
                    ensures: &[],
                    return_ty: &["Int".into()],
                    param_names: &["x".into()],
                    ir: Some(func),
                    enc_ctx,
                },
            );
            encoder
                .background_axioms
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    #[cfg(feature = "cvc5-verify")]
    fn native_ir_output(func: &IrFunction, enc_ctx: IrEncodeContext<'_>) -> String {
        use std::collections::HashMap;

        use crate::cvc5_encoder_state::default_cvc5_encoder_state;
        use crate::cvc5_ir_native::apply_ir_body_constraints_cvc5;

        let tm = cvc5::TermManager::new();
        let mut state = default_cvc5_encoder_state();
        let mut vars = HashMap::new();
        apply_ir_body_constraints_cvc5(&tm, func, &["x".into()], &mut vars, &mut state, enc_ctx);
        state
            .axioms
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn assert_all_backends_branch_inlined() {
        let (func, blocks) = branch_if_else_ir_fixture();
        let enc_ctx = IrEncodeContext::new(None, None, Some(&blocks));

        #[cfg(feature = "z3-verify")]
        {
            let out = z3_ir_output(&func, enc_ctx);
            assert_ir_blocks_inlined(&out, out.matches("(=").count());
        }

        #[cfg(not(feature = "cvc5-verify"))]
        {
            let shell = shell_ir_output(&func, enc_ctx);
            let shell_axioms = shell.lines().filter(|l| l.contains("(assert")).count();
            assert_ir_blocks_inlined(&shell, shell_axioms);
        }

        #[cfg(feature = "cvc5-verify")]
        {
            let out = native_ir_output(&func, enc_ctx);
            assert_ir_blocks_inlined(&out, out.matches("(=").count());
        }
    }

    fn assert_all_backends_missing_block_uf() {
        let func = branch_if_else_missing_blocks_fixture();
        let enc_ctx = IrEncodeContext::default();

        #[cfg(feature = "z3-verify")]
        {
            let out = z3_ir_output(&func, enc_ctx);
            assert_ir_blocks_missing_uf_fallback(&out);
        }

        #[cfg(not(feature = "cvc5-verify"))]
        {
            let shell = shell_ir_output(&func, enc_ctx);
            assert_ir_blocks_missing_uf_fallback(&shell);
        }

        #[cfg(feature = "cvc5-verify")]
        {
            let out = native_ir_output(&func, enc_ctx);
            assert_ir_blocks_missing_uf_fallback(&out);
        }
    }

    #[test]
    fn ir_parity_branch_if_else_inlining() {
        assert_all_backends_branch_inlined();
    }

    #[test]
    fn ir_parity_missing_blocks_uf_fallback() {
        assert_all_backends_missing_block_uf();
    }

    #[test]
    #[cfg(not(feature = "cvc5-verify"))]
    fn transition_ir_uses_state_uf_shell() {
        use crate::ir::parse_ir_module;

        let func = parse_ir_module(
            r#"
module ts {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = transition $0 to Active : Int
    $result = load $1 : Int
  }
}
"#,
        )
        .unwrap()
        .functions[0]
            .clone();

        let shell = shell_ir_output(&func, IrEncodeContext::default());
        assert!(
            shell.contains("__ir_state_Active"),
            "Transition should lower to state UF, got:\n{shell}"
        );

        #[cfg(feature = "z3-verify")]
        {
            let out = z3_ir_output(&func, IrEncodeContext::default());
            assert!(
                out.contains("__ir_state_Active"),
                "Z3 Transition UF missing:\n{out}"
            );
        }
    }

    #[test]
    #[cfg(not(feature = "cvc5-verify"))]
    fn construct_ir_untyped_uses_opaque_uf_shell() {
        use crate::ir::parse_ir_module;

        let func = parse_ir_module(
            r#"
module adt {
  fn #0 : ($0: Int) -> Pair ! pure
  {
    $result = construct Pair { .0 = $0, .1 = $0 } : Pair
  }
}
"#,
        )
        .unwrap()
        .functions[0]
            .clone();

        let shell = shell_ir_output(&func, IrEncodeContext::default());
        assert!(
            shell.contains("__ir_construct_Pair"),
            "untyped Construct should use opaque UF, got:\n{shell}"
        );
    }

    /// Verify Construct tag axiom is present in all backends (#303).
    #[test]
    fn construct_ir_result_tag_axiom_parity() {
        use crate::ir::parse_ir_module;

        let func = parse_ir_module(
            r#"
module adt {
  fn #0 : ($0: Int) -> MyStruct ! pure
  {
    $result = construct MyStruct { .0 = $0 } : MyStruct
  }
}
"#,
        )
        .unwrap()
        .functions[0]
            .clone();

        let enc_ctx = IrEncodeContext::default();

        // Shell backend should emit tag axiom (only compiled without cvc5-verify)
        #[cfg(not(feature = "cvc5-verify"))]
        {
            let shell = shell_ir_output(&func, enc_ctx);
            assert!(
                shell.contains("__ir_tag_MyStruct"),
                "shell backend should emit __ir_tag_MyStruct axiom, got:\n{shell}"
            );
        }

        // Z3 backend should emit tag axiom
        #[cfg(feature = "z3-verify")]
        {
            let out = z3_ir_output(&func, enc_ctx);
            assert!(
                out.contains("__ir_tag_MyStruct"),
                "Z3 backend should emit __ir_tag_MyStruct axiom, got:\n{out}"
            );
        }

        // CVC5 native backend should emit tag axiom
        #[cfg(feature = "cvc5-verify")]
        {
            let out = native_ir_output(&func, enc_ctx);
            assert!(
                out.contains("__ir_tag_MyStruct"),
                "CVC5 native backend should emit __ir_tag_MyStruct axiom, got:\n{out}"
            );
        }
    }
}
