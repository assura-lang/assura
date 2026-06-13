; Keywords
[
  "contract" "service" "type" "enum" "fn" "extern"
  "module" "project" "import" "as"
] @keyword

; Modifiers
[
  "ghost" "lemma" "pub" "mut"
] @keyword.modifier

; Clause keywords
[
  "requires" "ensures" "effects" "invariant" "modifies"
  "input" "output" "errors" "reads" "states"
  "operation" "query"
] @keyword.function

; Control flow
[
  "if" "then" "else" "match" "in"
] @keyword.control

; Quantifiers
[
  "forall" "exists"
] @keyword.control

; Built-in types
(builtin_type) @type.builtin

; Type definitions
(type_def name: (identifier) @type.definition)
(enum_def name: (identifier) @type.definition)
(contract_decl name: (identifier) @type.definition)
(service_decl name: (identifier) @type.definition)

; Function definitions
(fn_def name: (identifier) @function)
(extern_decl name: (identifier) @function)

; Function calls
(call_expr function: (identifier) @function.call)

; Parameters
(param name: (identifier) @variable.parameter)

; Field access
(field_access field: (identifier) @property)

; Identifiers
(identifier) @variable

; Literals
(number) @number
(string) @string
(boolean) @boolean

; Comments
(comment) @comment

; Operators
[
  "+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">="
  "&&" "||" "=>" "!" "." ":"
] @operator

; Punctuation
[ "(" ")" "[" "]" "{" "}" "<" ">" ] @punctuation.bracket
[ "," ";" ] @punctuation.delimiter