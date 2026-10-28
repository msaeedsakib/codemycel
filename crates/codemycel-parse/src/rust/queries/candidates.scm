(call_expression
  function: [(identifier) @cand.call
             (scoped_identifier name: (identifier) @cand.call)
             (field_expression field: (field_identifier) @cand.call)
             (generic_function
               function: [(identifier) @cand.call
                          (scoped_identifier name: (identifier) @cand.call)
                          (field_expression field: (field_identifier) @cand.call)])])

(_expression/identifier) @cand.value
(_expression/scoped_identifier name: (identifier) @cand.value)
(shorthand_field_initializer (identifier) @cand.value)
(_pattern/scoped_identifier name: (identifier) @cand.value)
(tuple_struct_pattern
  type: [(identifier) @cand.value
         (scoped_identifier name: (identifier) @cand.value)])

(token_tree (identifier) @cand.value)

(_type/type_identifier) @cand.type
(_type/scoped_type_identifier name: (type_identifier) @cand.type)
(_type/generic_type
  type: [(type_identifier) @cand.type
         (scoped_type_identifier name: (type_identifier) @cand.type)])
(dynamic_type
  trait: [(type_identifier) @cand.type
          (scoped_type_identifier name: (type_identifier) @cand.type)
          (generic_type
            type: [(type_identifier) @cand.type
                   (scoped_type_identifier name: (type_identifier) @cand.type)])])
(abstract_type
  trait: [(type_identifier) @cand.type
          (scoped_type_identifier name: (type_identifier) @cand.type)
          (generic_type
            type: [(type_identifier) @cand.type
                   (scoped_type_identifier name: (type_identifier) @cand.type)])])
(struct_expression
  name: [(type_identifier) @cand.type
         (scoped_type_identifier name: (type_identifier) @cand.type)
         (generic_type_with_turbofish type: (type_identifier) @cand.type)])
(struct_pattern
  type: [(type_identifier) @cand.type
         (scoped_type_identifier name: (type_identifier) @cand.type)])

(impl_item
  trait: [(type_identifier) @cand.implements
          (scoped_type_identifier name: (type_identifier) @cand.implements)
          (generic_type
            type: [(type_identifier) @cand.implements
                   (scoped_type_identifier name: (type_identifier) @cand.implements)])])

(trait_item
  bounds: (trait_bounds
    [(type_identifier) @cand.extends
     (scoped_type_identifier name: (type_identifier) @cand.extends)
     (generic_type
       type: [(type_identifier) @cand.extends
              (scoped_type_identifier name: (type_identifier) @cand.extends)])]))

(use_declaration
  argument: [(identifier) @cand.import
             (scoped_identifier name: (identifier) @cand.import)])
(use_list
  [(identifier) @cand.import
   (scoped_identifier name: (identifier) @cand.import)])
(use_as_clause
  path: [(identifier) @cand.import
         (scoped_identifier name: (identifier) @cand.import)])
(use_wildcard
  [(identifier) @cand.import
   (scoped_identifier name: (identifier) @cand.import)])
(extern_crate_declaration name: (identifier) @cand.import)
