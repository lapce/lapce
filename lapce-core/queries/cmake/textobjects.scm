; src: https://github.com/helix-editor/helix/blob/master/runtime/queries/cmake/textobjects.scm
; license: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

[
  (macro_def)
  (function_def)
] @function.around

(argument) @parameter.inside

[
  (bracket_comment)
  (line_comment)
] @comment.inside

(line_comment)+ @comment.around

(bracket_comment) @comment.around