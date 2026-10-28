(function_declaration
  name: (identifier) @def.name
  body: (statement_block) @def.body) @def.function

(generator_function_declaration
  name: (identifier) @def.name
  body: (statement_block) @def.body) @def.function

(export_statement
  "export" @def.visibility
  declaration: [(function_declaration) (generator_function_declaration)] @def.function)

(class_declaration
  name: (_) @def.name
  body: (class_body) @def.body) @def.class

(export_statement
  "export" @def.visibility
  declaration: (class_declaration) @def.class)

(class_body
  (method_definition
    name: (_) @def.name
    body: (statement_block) @def.body) @def.method)

(program
  (lexical_declaration
    kind: "const"
    (variable_declarator
      name: (identifier) @def.name
      value: (arrow_function body: (_) @def.body)) @def.function))

(program
  (lexical_declaration
    kind: "const"
    (variable_declarator
      name: (identifier) @def.name
      value: [(function_expression) (generator_function) (_ !parameters !parameter)] @def.body) @def.constant)
  (#not-match? @def.body "^require\\s*\\("))

(program
  (lexical_declaration
    kind: "const"
    (variable_declarator
      name: (identifier) @def.name
      !value) @def.constant))

(program
  (lexical_declaration
    kind: "let"
    (variable_declarator
      name: (identifier) @def.name
      value: (_)? @def.body) @def.variable)
  (#not-match? @def.body "^require\\s*\\("))

(program
  (variable_declaration
    (variable_declarator
      name: (identifier) @def.name
      value: (_)? @def.body) @def.variable)
  (#not-match? @def.body "^require\\s*\\("))

(export_statement
  "export" @def.visibility
  declaration: (lexical_declaration
    kind: "const"
    (variable_declarator
      name: (identifier) @def.name
      value: (arrow_function body: (_) @def.body)) @def.function))

(export_statement
  "export" @def.visibility
  declaration: (lexical_declaration
    kind: "const"
    (variable_declarator
      name: (identifier) @def.name
      value: [(function_expression) (generator_function) (_ !parameters !parameter)] @def.body) @def.constant)
  (#not-match? @def.body "^require\\s*\\("))

(export_statement
  "export" @def.visibility
  declaration: (lexical_declaration
    kind: "const"
    (variable_declarator
      name: (identifier) @def.name
      !value) @def.constant))

(export_statement
  "export" @def.visibility
  declaration: (lexical_declaration
    kind: "let"
    (variable_declarator
      name: (identifier) @def.name
      value: (_)? @def.body) @def.variable)
  (#not-match? @def.body "^require\\s*\\("))

(export_statement
  "export" @def.visibility
  declaration: (variable_declaration
    (variable_declarator
      name: (identifier) @def.name
      value: (_)? @def.body) @def.variable)
  (#not-match? @def.body "^require\\s*\\("))
