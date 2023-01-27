; source: https://github.com/nvim-treesitter/nvim-treesitter/blob/965a74f76a2999b81fe3a543fb5e53bf6c84b8b7/queries/latex/injections.scm
; licence: https://github.com/nvim-treesitter/nvim-treesitter/blob/965a74f76a2999b81fe3a543fb5e53bf6c84b8b7/LICENSE
; spdx: Apache-2.0

[
 (line_comment)
 (block_comment)
 (comment_environment)
] @comment

(pycode_environment
  code: (source_code) @python
)

(minted_environment
  (begin
    language: (curly_group_text
               (text) @language))
  (source_code) @content)

((generic_environment
  (begin
   name: (curly_group_text
           (text) @_env))) @c
   (#any-of? @_env "asy" "asydef"))
