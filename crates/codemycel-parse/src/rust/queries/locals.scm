[
  (block)
  (function_item)
  (function_signature_item)
  (closure_expression)
  (match_arm)
  (for_expression)
  (if_expression)
  (while_expression)
  (impl_item)
  (trait_item)
  (struct_item)
  (enum_item)
  (union_item)
  (type_item)
] @local.scope

((_pattern/identifier) @local.definition
  (#not-match? @local.definition "^\\p{Lu}"))
(field_pattern name: (shorthand_field_identifier) @local.definition)
(type_parameter name: (type_identifier) @local.definition)
(const_parameter name: (identifier) @local.definition)

(let_declaration value: (_) @local.exclude)
(let_condition value: (_) @local.exclude)
(for_expression value: (_) @local.exclude)
