(function_declaration return_type: (_) @def.return_type) @def.function

(generator_function_declaration return_type: (_) @def.return_type) @def.function

(class_body
  (method_definition return_type: (_) @def.return_type) @def.method)

(variable_declarator
  value: (arrow_function return_type: (_) @def.return_type)) @def.function

(function_signature
  name: (identifier) @def.name
  return_type: (_)? @def.return_type) @def.function @def.overload

(export_statement
  "export" @def.visibility
  declaration: (function_signature) @def.function)

(abstract_class_declaration
  name: (type_identifier) @def.name
  body: (class_body) @def.body) @def.class

(export_statement
  "export" @def.visibility
  declaration: (abstract_class_declaration) @def.class)

(class_body
  [(method_signature
     name: (_) @def.name
     return_type: (_)? @def.return_type)
   (abstract_method_signature
     name: (_) @def.name
     return_type: (_)? @def.return_type)] @def.method @def.overload)

(class_body
  (public_field_definition
    name: (_) @def.name
    value: [
      (arrow_function return_type: (_)? @def.return_type body: (_) @def.body)
      (function_expression return_type: (_)? @def.return_type body: (_) @def.body)
      (generator_function return_type: (_)? @def.return_type body: (_) @def.body)
    ]) @def.method)

(class_body
  (public_field_definition
    name: (_) @def.name
    value: (_ !parameters !parameter) @def.body) @def.field)

(class_body
  (public_field_definition
    name: (_) @def.name
    !value) @def.field)

(interface_declaration
  name: (type_identifier) @def.name
  body: (interface_body) @def.body) @def.interface

(export_statement
  "export" @def.visibility
  declaration: (interface_declaration) @def.interface)

(interface_body
  (property_signature name: (_) @def.name) @def.field)

(interface_body
  (method_signature
    name: (_) @def.name
    return_type: (_)? @def.return_type) @def.method @def.overload)

(enum_declaration
  name: (identifier) @def.name
  body: (enum_body) @def.body) @def.enum

(export_statement
  "export" @def.visibility
  declaration: (enum_declaration) @def.enum)

(enum_body name: (_) @def.name @def.enum_member)

(enum_assignment name: (_) @def.name) @def.enum_member

(type_alias_declaration name: (type_identifier) @def.name) @def.type_alias

(export_statement
  "export" @def.visibility
  declaration: (type_alias_declaration) @def.type_alias)

(internal_module
  name: (_) @def.name
  body: (statement_block) @def.body) @def.module

(module
  name: (_) @def.name
  body: (statement_block)? @def.body) @def.module

(export_statement
  "export" @def.visibility
  declaration: (internal_module) @def.module)

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration
    [(function_signature) (function_declaration)] @def.function))

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration
    [(class_declaration) (abstract_class_declaration)] @def.class))

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration (interface_declaration) @def.interface))

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration (enum_declaration) @def.enum))

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration (type_alias_declaration) @def.type_alias))

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration [(internal_module) (module)] @def.module))

(ambient_declaration
  (lexical_declaration
    kind: "const"
    (variable_declarator name: (identifier) @def.name) @def.constant))

(ambient_declaration
  (lexical_declaration
    kind: "let"
    (variable_declarator name: (identifier) @def.name) @def.variable))

(ambient_declaration
  (variable_declaration
    (variable_declarator name: (identifier) @def.name) @def.variable))

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration
    (lexical_declaration
      kind: "const"
      (variable_declarator) @def.constant)))

(export_statement
  "export" @def.visibility
  declaration: (ambient_declaration
    [(lexical_declaration kind: "let" (variable_declarator) @def.variable)
     (variable_declaration (variable_declarator) @def.variable)]))

(internal_module
  body: (statement_block
    (lexical_declaration
      kind: "const"
      (variable_declarator
        name: (identifier) @def.name
        value: (arrow_function body: (_) @def.body)) @def.function)))

(internal_module
  body: (statement_block
    (lexical_declaration
      kind: "const"
      (variable_declarator
        name: (identifier) @def.name
        value: [(function_expression) (generator_function) (_ !parameters !parameter)] @def.body) @def.constant))
  (#not-match? @def.body "^require\\s*\\("))

(internal_module
  body: (statement_block
    (lexical_declaration
      kind: "const"
      (variable_declarator
        name: (identifier) @def.name
        !value) @def.constant)))

(internal_module
  body: (statement_block
    [(lexical_declaration
       kind: "let"
       (variable_declarator
         name: (identifier) @def.name
         value: (_)? @def.body) @def.variable)
     (variable_declaration
       (variable_declarator
         name: (identifier) @def.name
         value: (_)? @def.body) @def.variable)])
  (#not-match? @def.body "^require\\s*\\("))
