; src: https://github.com/helix-editor/helix/blob/master/runtime/queries/cmake/indents.scm
; license: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

[
  (if_condition)
  (foreach_loop)
  (while_loop)
  (function_def)
  (macro_def)
  (normal_command)
] @indent

")" @outdent