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
  "if" "then" "else" "in"
] @keyword.control

; Quantifiers
[
  "forall" "exists"
] @keyword.control

; Built-in types
(builtin_type) @type.builtin

; Type definitions
(type_def (identifier) @type.definition)
(enum_def (identifier) @type.definition)
(contract_decl (identifier) @type.definition)
(service_decl (identifier) @type.definition)

; Function definitions
(fn_def (identifier) @function)
(extern_decl (identifier) @function)

; Parameters
(param (identifier) @variable.parameter)

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