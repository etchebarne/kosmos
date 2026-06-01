; JSX tags and attributes
;------------------------

(jsx_opening_element
  name: (identifier) @tag
  (#match? @tag "^[a-z][^.]*$"))

(jsx_closing_element
  name: (identifier) @tag
  (#match? @tag "^[a-z][^.]*$"))

(jsx_self_closing_element
  name: (identifier) @tag
  (#match? @tag "^[a-z][^.]*$"))

(jsx_attribute
  (property_identifier) @attribute)

(jsx_text) @text.literal

(jsx_opening_element
  ["<" ">"] @punctuation.bracket)

(jsx_closing_element
  ["</" ">"] @punctuation.bracket)

(jsx_self_closing_element
  ["<" "/>" ] @punctuation.bracket)

(jsx_expression
  ["{" "}"] @punctuation.bracket)
