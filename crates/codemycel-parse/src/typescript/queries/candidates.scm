(call_expression
  function: (identifier) @cand.call
  (#not-eq? @cand.call "require"))

(call_expression
  function: (member_expression property: (property_identifier) @cand.call))

(new_expression constructor: (identifier) @cand.call)

(new_expression
  constructor: (member_expression property: (property_identifier) @cand.call))

((primary_expression/identifier) @cand.value
  (#not-eq? @cand.value "require"))

(shorthand_property_identifier) @cand.value

(assignment_expression left: (identifier) @cand.value)

(augmented_assignment_expression left: (identifier) @cand.value)

(import_specifier name: (identifier) @cand.import)

(import_clause (identifier) @cand.import)

(namespace_import (identifier) @cand.import)

(import_statement
  .
  source: (string (string_fragment) @cand.import))

(variable_declarator
  name: [
    (identifier) @cand.import
    (object_pattern (shorthand_property_identifier_pattern) @cand.import)
    (object_pattern (pair_pattern key: (property_identifier) @cand.import))
  ]
  value: (call_expression function: (identifier) @_require)
  (#eq? @_require "require"))

(export_specifier name: (identifier) @cand.export)

(export_statement value: (identifier) @cand.export)

(export_statement
  "*"
  source: (string (string_fragment) @cand.export))

(export_statement
  (namespace_export)
  source: (string (string_fragment) @cand.export))
