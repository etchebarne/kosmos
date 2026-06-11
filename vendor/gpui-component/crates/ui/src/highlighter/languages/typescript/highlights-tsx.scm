; JSX tags and attributes
;------------------------

[
  (jsx_opening_element
    name: (_) @tag)

  (jsx_closing_element
    name: (_) @tag)

  (jsx_self_closing_element
    name: (_) @tag)
]

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
