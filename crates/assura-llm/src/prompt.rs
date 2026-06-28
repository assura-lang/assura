//! Prompt templates and response parsing for LLM analysis.

use crate::types::*;

const PROMPT_VERSION: &str = "1";

/// Returns the prompt version for cache keying.
pub fn prompt_version() -> &'static str {
    PROMPT_VERSION
}

// ---------------------------------------------------------------------------
// Level 1: body-vs-contract analysis
// ---------------------------------------------------------------------------

pub fn analysis_system_prompt() -> String {
    r#"You are a contract verification assistant for Rust code. Your task is to analyze whether a Rust function body satisfies its contract annotations (#[requires], #[ensures], #[invariant]).

You must respond ONLY with a JSON object (no markdown fences, no prose before or after). The JSON must have this exact structure:

{
  "verdict": "pass" | "fail" | "uncertain",
  "confidence": <float 0.0 to 1.0>,
  "paths": [
    {
      "description": "<brief description of the control-flow path>",
      "reachable_given_preconditions": true | false,
      "contracts_satisfied": true | false,
      "reasoning": "<why this path does or does not satisfy the contracts>"
    }
  ],
  "violations": [
    {
      "clause_kind": "requires" | "ensures" | "invariant",
      "clause_expression": "<the contract expression>",
      "description": "<what went wrong>",
      "evidence_line": <line number or null>
    }
  ],
  "reasoning": "<overall reasoning>"
}

Rules:
- For each #[requires] clause: verify the function body correctly assumes this precondition.
- For each #[ensures] clause: verify EVERY return path satisfies this postcondition. Consider early returns, ? operator, and the final expression.
- For each #[invariant] clause: verify the property is maintained by every mutation.
- "result" in ensures refers to the return value.
- "old(expr)" in ensures refers to the value of expr before the function body executes.
- If you cannot determine the verdict, use "uncertain".
- Do NOT suggest code improvements. Only analyze contract compliance.
- When called functions have known contracts, use their #[ensures] as known facts after the call."#.to_string()
}

pub fn analysis_user_prompt(req: &AnalysisRequest) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str("## Function\n\n```rust\n");
    prompt.push_str(&req.function_signature);
    prompt.push_str(" {\n");
    prompt.push_str(&req.function_body);
    prompt.push_str("\n}\n```\n\n");

    prompt.push_str("## Contracts\n\n");
    if req.contracts.is_empty() {
        prompt.push_str("(none)\n\n");
    } else {
        for c in &req.contracts {
            prompt.push_str(&format!("- #[{}({})]\n", c.kind, c.expression));
        }
        prompt.push('\n');
    }

    if !req.context.called_functions.is_empty() {
        prompt.push_str("## Called Functions with Contracts\n\n");
        for cf in &req.context.called_functions {
            prompt.push_str(&format!("**{}** ({})\n", cf.name, cf.source_file));
            prompt.push_str(&format!("  Signature: {}\n", cf.signature));
            for r in &cf.requires {
                prompt.push_str(&format!("  #[requires({r})]\n"));
            }
            for e in &cf.ensures {
                prompt.push_str(&format!("  #[ensures({e})]\n"));
            }
            prompt.push('\n');
        }
    }

    if !req.context.surrounding_types.is_empty() {
        prompt.push_str("## Type Definitions\n\n");
        for t in &req.context.surrounding_types {
            prompt.push_str(&format!("```rust\n{}\n```\n\n", t.definition));
        }
    }

    prompt.push_str(
        "Analyze whether the function body satisfies all contracts. Respond with JSON only.\n",
    );
    prompt
}

// ---------------------------------------------------------------------------
// Suggestion: propose contracts for unannotated functions
// ---------------------------------------------------------------------------

pub fn suggestion_system_prompt() -> String {
    r#"You are a contract suggestion assistant for Rust code. Your task is to propose #[requires], #[ensures], #[ensures_ok], #[ensures_err], and #[invariant] annotations for Rust functions that have no contracts.

You must respond ONLY with a JSON object (no markdown fences, no prose before or after):

{
  "suggestions": [
    {
      "kind": "requires" | "ensures" | "ensures_ok" | "ensures_err" | "invariant",
      "expression": "<Rust expression for the contract>",
      "confidence": <float 0.0 to 1.0>,
      "reasoning": "<why you propose this contract>",
      "evidence_line": <line number in the function body or null>
    }
  ],
  "skipped_reason": null | "<reason if you cannot suggest contracts>"
}

Rules:
- Only propose contracts you are confident about (>= 0.8 confidence).
- Prefer fewer, stronger contracts over many weak ones.
- Use the function's parameter names in expressions.
- Use "result" for the return value.
- For Result<T,E> functions, prefer #[ensures_ok] and #[ensures_err] over #[ensures].
- Do NOT propose contracts that merely restate the type system.
- DO propose contracts that capture domain logic the type system misses.
- Look for: guard clauses, assertions, bounds checks, division, null/None checks, domain constraints.
- Do NOT suggest code changes or refactoring."#.to_string()
}

pub fn suggestion_user_prompt(req: &SuggestionRequest) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str("## Function\n\n```rust\n");
    if !req.doc_comments.is_empty() {
        for line in req.doc_comments.lines() {
            prompt.push_str(&format!("/// {line}\n"));
        }
    }
    prompt.push_str(&req.function_signature);
    prompt.push_str(" {\n");
    prompt.push_str(&req.function_body);
    prompt.push_str("\n}\n```\n\n");

    if let Some(ty) = &req.impl_type {
        prompt.push_str(&format!("This function is a method on `{ty}`.\n"));
    }
    prompt.push_str(&format!("Visibility: {}\n", req.visibility));
    if req.is_unsafe {
        prompt.push_str("This function is `unsafe`.\n");
    }
    if req.is_async {
        prompt.push_str("This function is `async`.\n");
    }
    prompt.push('\n');

    if !req.context.sibling_contracts.is_empty() {
        prompt.push_str("## Other functions in this module with contracts\n\n");
        for sc in &req.context.sibling_contracts {
            prompt.push_str(&format!("  {}\n", sc.signature));
            for r in &sc.requires {
                prompt.push_str(&format!("    #[requires({r})]\n"));
            }
            for e in &sc.ensures {
                prompt.push_str(&format!("    #[ensures({e})]\n"));
            }
        }
        prompt.push_str("\nMaintain consistent style with these existing contracts.\n\n");
    }

    prompt.push_str("Propose contracts for this function. Respond with JSON only.\n");
    prompt
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Extract JSON from LLM response text (handles markdown fences).
pub fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    // Strip ```json ... ``` if present
    if let Some(start) = trimmed.find('{') {
        let end = trimmed.rfind('}').unwrap_or(trimmed.len() - 1);
        &trimmed[start..=end]
    } else {
        trimmed
    }
}

pub fn parse_analysis_response(raw: &str) -> Result<AnalysisResponse, LlmError> {
    let json_str = extract_json(raw);

    let v: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| LlmError::Parse(format!("{e}: {json_str}")))?;

    let verdict_str = v["verdict"].as_str().unwrap_or("uncertain");
    let confidence = v["confidence"].as_f64().unwrap_or(0.5);
    let reasoning = v["reasoning"].as_str().unwrap_or("").to_string();

    let paths: Vec<PathAnalysis> = v["paths"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| serde_json::from_value(p.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    let verdict = match verdict_str {
        "pass" => Verdict::Pass,
        "fail" => {
            let violations: Vec<Violation> = v["violations"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|vi| serde_json::from_value(vi.clone()).ok())
                        .collect()
                })
                .unwrap_or_default();
            Verdict::Fail { violations }
        }
        _ => Verdict::Uncertain {
            reason: reasoning.clone(),
        },
    };

    Ok(AnalysisResponse {
        verdict,
        confidence,
        paths,
        reasoning,
    })
}

pub fn parse_suggestion_response(raw: &str) -> Result<SuggestionResponse, LlmError> {
    let json_str = extract_json(raw);
    serde_json::from_str(json_str).map_err(|e| LlmError::Parse(format!("{e}: {json_str}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_analysis_pass() {
        let raw = r#"{"verdict":"pass","confidence":0.95,"paths":[],"reasoning":"all good","violations":[]}"#;
        let resp = parse_analysis_response(raw).unwrap();
        assert!(matches!(resp.verdict, Verdict::Pass));
        assert!((resp.confidence - 0.95).abs() < 0.01);
    }

    #[test]
    fn parse_analysis_fail() {
        let raw = r#"{"verdict":"fail","confidence":0.9,"paths":[],"reasoning":"bug","violations":[{"clause_kind":"ensures","clause_expression":"result > 0","description":"returns 0","evidence_line":5}]}"#;
        let resp = parse_analysis_response(raw).unwrap();
        assert!(matches!(resp.verdict, Verdict::Fail { .. }));
        if let Verdict::Fail { violations } = &resp.verdict {
            assert_eq!(violations.len(), 1);
            assert_eq!(violations[0].clause_expression, "result > 0");
        }
    }

    #[test]
    fn parse_analysis_with_markdown_fences() {
        let raw = "```json\n{\"verdict\":\"pass\",\"confidence\":1.0,\"paths\":[],\"reasoning\":\"ok\",\"violations\":[]}\n```";
        let resp = parse_analysis_response(raw).unwrap();
        assert!(matches!(resp.verdict, Verdict::Pass));
    }

    #[test]
    fn parse_suggestion_response_ok() {
        let raw = r#"{"suggestions":[{"kind":"requires","expression":"x > 0","confidence":0.95,"reasoning":"guard clause","evidence_line":3}],"skipped_reason":null}"#;
        let resp = parse_suggestion_response(raw).unwrap();
        assert_eq!(resp.suggestions.len(), 1);
        assert_eq!(resp.suggestions[0].expression, "x > 0");
    }

    #[test]
    fn extract_json_strips_fences() {
        let raw = "Here is the JSON:\n```json\n{\"a\":1}\n```\nDone.";
        assert_eq!(extract_json(raw), "{\"a\":1}");
    }

    #[test]
    fn parse_analysis_invalid_json() {
        let result = parse_analysis_response("this is not json");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LlmError::Parse(_)));
    }

    #[test]
    fn parse_suggestion_invalid_json() {
        let result = parse_suggestion_response("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn extract_json_no_braces() {
        let result = extract_json("no json here");
        assert_eq!(result, "no json here");
    }
}
