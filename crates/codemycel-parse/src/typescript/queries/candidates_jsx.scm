([(jsx_opening_element name: (identifier) @cand.call)
  (jsx_self_closing_element name: (identifier) @cand.call)]
  (#match? @cand.call "^[A-Z]"))

[(jsx_opening_element
   name: (member_expression property: (property_identifier) @cand.call))
 (jsx_self_closing_element
   name: (member_expression property: (property_identifier) @cand.call))]
