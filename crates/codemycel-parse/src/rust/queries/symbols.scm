(mod_item
  (visibility_modifier)? @def.visibility
  name: (identifier) @def.name
  body: (declaration_list)? @def.body) @def.module

(struct_item
  (visibility_modifier)? @def.visibility
  name: (type_identifier) @def.name
  body: (_)? @def.body) @def.struct

(union_item
  (visibility_modifier)? @def.visibility
  name: (type_identifier) @def.name
  body: (field_declaration_list) @def.body) @def.struct

(enum_item
  (visibility_modifier)? @def.visibility
  name: (type_identifier) @def.name
  body: (enum_variant_list) @def.body) @def.enum

(enum_variant
  (visibility_modifier)? @def.visibility
  name: (identifier) @def.name
  body: (_)? @def.body) @def.enum_member

(field_declaration
  (visibility_modifier)? @def.visibility
  name: (field_identifier) @def.name) @def.field

(trait_item
  (visibility_modifier)? @def.visibility
  name: (type_identifier) @def.name
  body: (declaration_list) @def.body) @def.trait

(type_item
  (visibility_modifier)? @def.visibility
  name: (type_identifier) @def.name) @def.type_alias

(associated_type
  name: (type_identifier) @def.name) @def.type_alias

(const_item
  (visibility_modifier)? @def.visibility
  name: (identifier) @def.name
  value: (_)? @def.body) @def.constant

(static_item
  (visibility_modifier)? @def.visibility
  name: (identifier) @def.name
  value: (_)? @def.body) @def.constant

(source_file
  (function_item
    (visibility_modifier)? @def.visibility
    name: (identifier) @def.name
    return_type: (_)? @def.return_type
    body: (block) @def.body) @def.function)

(block
  (function_item
    (visibility_modifier)? @def.visibility
    name: (identifier) @def.name
    return_type: (_)? @def.return_type
    body: (block) @def.body) @def.function)

(mod_item
  body: (declaration_list
    (function_item
      (visibility_modifier)? @def.visibility
      name: (identifier) @def.name
      return_type: (_)? @def.return_type
      body: (block) @def.body) @def.function))

(foreign_mod_item
  body: (declaration_list
    (function_signature_item
      (visibility_modifier)? @def.visibility
      name: (identifier) @def.name
      return_type: (_)? @def.return_type) @def.function))

(impl_item
  body: (declaration_list
    (function_item
      (visibility_modifier)? @def.visibility
      name: (identifier) @def.name
      return_type: (_)? @def.return_type
      body: (block) @def.body) @def.method))

(trait_item
  body: (declaration_list
    (function_item
      (visibility_modifier)? @def.visibility
      name: (identifier) @def.name
      return_type: (_)? @def.return_type
      body: (block) @def.body) @def.method))

(trait_item
  body: (declaration_list
    (function_signature_item
      (visibility_modifier)? @def.visibility
      name: (identifier) @def.name
      return_type: (_)? @def.return_type) @def.method))

(impl_item
  trait: (_)? @impl.trait
  type: (_) @impl.type
  body: (declaration_list) @impl.body) @impl

(impl_item
  type: [(scoped_type_identifier name: (type_identifier) @impl.type_name)
         (generic_type type: [(type_identifier) @impl.type_name
                              (scoped_type_identifier name: (type_identifier) @impl.type_name)])]) @impl
