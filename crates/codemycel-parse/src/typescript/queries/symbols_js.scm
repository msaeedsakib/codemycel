(class_body
  (field_definition
    property: (_) @def.name
    value: [
      (arrow_function body: (_) @def.body)
      (function_expression body: (_) @def.body)
      (generator_function body: (_) @def.body)
    ]) @def.method)

(class_body
  (field_definition
    property: (_) @def.name
    value: (_ !parameters !parameter) @def.body) @def.field)

(class_body
  (field_definition
    property: (_) @def.name
    !value) @def.field)
