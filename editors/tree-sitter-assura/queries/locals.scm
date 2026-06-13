; Scopes
(contract_decl) @scope
(service_decl) @scope
(fn_def) @scope
(fn_body) @scope
(block_expr) @scope

; Definitions
(type_def name: (identifier) @definition.type)
(enum_def name: (identifier) @definition.type)
(fn_def name: (identifier) @definition.function)
(param name: (identifier) @definition.parameter)
(field_def name: (identifier) @definition.field)

; References
(identifier) @reference