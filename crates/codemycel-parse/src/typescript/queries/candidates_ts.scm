(primary_type/type_identifier) @cand.type

(generic_type name: (type_identifier) @cand.type)

(nested_type_identifier name: (type_identifier) @cand.type)

(type_query (identifier) @cand.value)

(extends_clause value: (identifier) @cand.extends)

(extends_clause
  value: (member_expression property: (property_identifier) @cand.extends))

(extends_type_clause
  type: [
    (type_identifier) @cand.extends
    (nested_type_identifier name: (type_identifier) @cand.extends)
    (generic_type name: (type_identifier) @cand.extends)
    (generic_type name: (nested_type_identifier name: (type_identifier) @cand.extends))
  ])

(implements_clause
  [
    (type_identifier) @cand.implements
    (nested_type_identifier name: (type_identifier) @cand.implements)
    (generic_type name: (type_identifier) @cand.implements)
    (generic_type name: (nested_type_identifier name: (type_identifier) @cand.implements))
  ])
