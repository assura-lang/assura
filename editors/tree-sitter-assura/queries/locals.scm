; Scopes
(contract_decl) @scope
(service_decl) @scope
(fn_def) @scope
(fn_body) @scope
(block_expr) @scope

; Definitions
(type_def (identifier) @definition.type)
(enum_def (identifier) @definition.type)
(fn_def (identifier) @definition.function)
(param (identifier) @definition.parameter)
(field_def (identifier) @definition.field)

; References
(identifier) @reference