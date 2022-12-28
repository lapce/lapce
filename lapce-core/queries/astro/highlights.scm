; source: https://github.com/virchau13/tree-sitter-astro/blob/22697b0e2413464b7abaea9269c5e83a59e39a83/queries/highlights.scm
; licence: https://github.com/virchau13/tree-sitter-astro/blob/22697b0e2413464b7abaea9269c5e83a59e39a83/LICENSE
; spdx: MIT

(tag_name) @tag
(erroneous_end_tag_name) @tag.error
(doctype) @constant
(attribute_name) @attribute
(attribute_value) @string
(comment) @comment

[
  "<"
  ">"
  "</"
] @punctuation.bracket