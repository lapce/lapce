;src: https://github.com/nvim-treesitter/nvim-treesitter/blob/master/queries/svelte/indents.scm
;licence https://github.com/nvim-treesitter/nvim-treesitter/blob/master/LICENSE
; spdx: Apache-2.0

[
  (element)
  (if_statement)
  (each_statement)
  (await_statement)
  (script_element)
  (style_element)
] @indent

[
  (end_tag)
  (else_statement)
  (if_end_expr)
  (each_end_expr)
  (await_end_expr)
  ">"
  "/>"
] @branch

(comment) @ignore
