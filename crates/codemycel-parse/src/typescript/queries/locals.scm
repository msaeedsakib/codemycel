[
  (function_declaration)
  (generator_function_declaration)
  (function_expression)
  (generator_function)
  (arrow_function)
  (method_definition)
  (class_declaration)
  (class)
  (class_static_block)
  (if_statement)
  (else_clause)
  (for_statement)
  (for_in_statement)
  (while_statement)
  (do_statement)
  (try_statement)
  (catch_clause)
  (finally_clause)
  (switch_statement)
  (labeled_statement)
] @local.scope

(program (statement_block) @local.scope)

(statement_block (statement_block) @local.scope)

(variable_declarator name: (identifier) @local.definition)

(pattern/identifier) @local.definition

(shorthand_property_identifier_pattern) @local.definition

(arrow_function parameter: (identifier) @local.definition)

(catch_clause parameter: (identifier) @local.definition)

(for_in_statement kind: _ left: (identifier) @local.definition)

(assignment_expression left: [(array_pattern) (object_pattern)] @local.scope)

(for_in_statement !kind left: [(array_pattern) (object_pattern)] @local.scope)

(if_statement alternative: (else_clause) @local.exclude)

(catch_clause) @local.exclude

(finally_clause) @local.exclude
