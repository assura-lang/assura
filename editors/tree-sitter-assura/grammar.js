/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: "assura",

  extras: ($) => [/\s/, $.comment],

  conflicts: ($) => [[$.if_expr]],

  word: ($) => $.identifier,

  rules: {
    source_file: ($) =>
      repeat(
        choice(
          $.project_decl,
          $.module_decl,
          $.import_decl,
          $.contract_decl,
          $.service_decl,
          $.type_def,
          $.enum_def,
          $.extern_decl,
          $.fn_def,
          $.block_decl,
        ),
      ),

    // Top-level declarations
    project_decl: ($) =>
      seq("project", $.identifier, "{", repeat($.project_item), "}"),
    project_item: ($) => seq($.identifier, ":", $.expression),

    module_decl: ($) => seq("module", $.dotted_name, ";"),
    import_decl: ($) =>
      seq(
        "import",
        $.dotted_name,
        optional(choice(seq("as", $.identifier), seq("{", commaSep($.identifier), "}"))),
        optional(";"),
      ),

    // Contract
    contract_decl: ($) =>
      seq(
        "contract",
        $.identifier,
        optional($.type_params),
        "{",
        repeat($.clause),
        "}",
      ),

    clause: ($) =>
      seq(
        choice(
          "requires",
          "ensures",
          "effects",
          "invariant",
          "modifies",
          "input",
          "output",
          "errors",
          "reads",
        ),
        "{",
        repeat($.expression),
        "}",
      ),

    // Service
    service_decl: ($) =>
      seq("service", $.identifier, "{", repeat($.service_item), "}"),
    service_item: ($) =>
      choice(
        $.type_def,
        $.enum_def,
        seq("states", "{", commaSep($.identifier), "}"),
        seq("operation", $.identifier, "{", repeat($.clause), "}"),
        seq("query", $.identifier, "{", repeat($.clause), "}"),
        seq("invariant", "{", repeat($.expression), "}"),
      ),

    // Type and enum definitions
    type_def: ($) =>
      seq("type", $.identifier, optional($.type_params), $.type_body),
    type_body: ($) =>
      choice(seq("=", $.type_ref), seq("{", repeat($.field_def), "}")),
    field_def: ($) =>
      seq(optional("pub"), $.identifier, ":", $.type_ref, optional(",")),

    enum_def: ($) =>
      seq(
        "enum",
        $.identifier,
        optional($.type_params),
        "{",
        commaSep($.enum_variant),
        "}",
      ),
    enum_variant: ($) =>
      seq($.identifier, optional(seq("(", commaSep($.type_ref), ")"))),

    // Extern and function definitions
    extern_decl: ($) =>
      seq(
        "extern",
        $.identifier,
        "(",
        commaSep($.param),
        ")",
        optional(seq("->", $.type_ref)),
        repeat($.clause),
      ),

    fn_def: ($) =>
      seq(
        optional("ghost"),
        optional("lemma"),
        "fn",
        $.identifier,
        "(",
        commaSep($.param),
        ")",
        optional(seq("->", $.type_ref)),
        repeat($.clause),
        optional($.fn_body),
      ),
    fn_body: ($) => seq("{", repeat($.expression), "}"),
    param: ($) => seq($.identifier, ":", $.type_ref),

    // Block declarations (feature, axiom, spec, etc.)
    block_decl: ($) =>
      seq($.identifier, $.identifier, optional($.block_body)),
    block_body: ($) =>
      choice(
        seq("{", repeat(choice($.expression, $.clause)), "}"),
        seq("=", $.expression),
      ),

    // Type references
    type_ref: ($) =>
      choice(
        seq($.builtin_type, optional(seq("<", commaSep($.type_ref), ">"))),
        seq($.identifier, optional(seq("<", commaSep($.type_ref), ">"))),
        seq("{", $.identifier, ":", $.type_ref, "|", $.expression, "}"),
        seq("&", optional("mut"), $.type_ref),
      ),

    builtin_type: (_) =>
      choice(
        "Int", "Nat", "Float", "Bool", "String", "Bytes", "Unit", "Never",
        "U8", "U16", "U32", "U64", "I8", "I16", "I32", "I64", "F32", "F64",
        "List", "Map", "Set", "Option", "Result",
      ),

    type_params: ($) => seq("<", commaSep($.identifier), ">"),

    // Expressions
    expression: ($) =>
      choice(
        $.literal,
        $.identifier,
        $.field_access,
        $.call_expr,
        $.index_expr,
        $.binary_expr,
        $.unary_expr,
        $.paren_expr,
        $.list_expr,
        $.if_expr,
        $.quantifier,
        $.old_expr,
        $.block_expr,
      ),

    literal: ($) => choice($.number, $.string, $.boolean),
    number: (_) => /\d[\d_]*(\.\d[\d_]*)?/,
    string: (_) => /"([^"\\]|\\.)*"/,
    boolean: (_) => choice("true", "false"),

    field_access: ($) => prec.left(10, seq($.expression, ".", $.identifier)),
    call_expr: ($) =>
      prec.left(9, seq($.expression, "(", commaSep($.expression), ")")),
    index_expr: ($) =>
      prec.left(9, seq($.expression, "[", $.expression, "]")),

    binary_expr: ($) =>
      choice(
        ...[
          ["||", 1],
          ["&&", 2],
          ["=>", 2],
          ["==", 3], ["!=", 3],
          ["<", 4], [">", 4], ["<=", 4], [">=", 4],
          ["+", 5], ["-", 5],
          ["*", 6], ["/", 6], ["%", 6],
          ["in", 3],
        ].map(([op, prec_val]) =>
          prec.left(Number(prec_val), seq($.expression, op, $.expression)),
        ),
      ),

    unary_expr: ($) =>
      prec(8, seq(choice("!", "-"), $.expression)),

    paren_expr: ($) => seq("(", $.expression, ")"),
    list_expr: ($) => seq("[", commaSep($.expression), "]"),
    if_expr: ($) =>
      seq("if", $.expression, "then", $.expression, optional(seq("else", $.expression))),
    quantifier: ($) =>
      seq(choice("forall", "exists"), $.identifier, "in", $.expression, ":", $.expression),
    old_expr: ($) => seq("old", "(", $.expression, ")"),
    block_expr: ($) => seq("{", repeat($.expression), "}"),

    // Names
    dotted_name: ($) => sep1($.identifier, "."),
    identifier: (_) => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // Comments
    comment: (_) =>
      token(
        choice(seq("//", /.*/), seq("/*", /[^*]*\*+([^/*][^*]*\*+)*/, "/")),
      ),
  },
});

/**
 * @param {RuleOrLiteral} rule
 */
function commaSep(rule) {
  return optional(sep1(rule, ","));
}

/**
 * @param {RuleOrLiteral} rule
 * @param {RuleOrLiteral} sep
 */
function sep1(rule, sep) {
  return seq(rule, repeat(seq(sep, rule)));
}