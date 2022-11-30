; source: https://github.com/helix-editor/helix/blob/master/runtime/queries/bash/highlights.scm
; licence: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

[
  (string)
  (raw_string)
  (heredoc_body)
  (heredoc_start)
] @string

(command_name) @function

(variable_name) @variable.other.member

[
  "case"
  "do"
  "done"
  "elif"
  "else"
  "esac"
  "export"
  "fi"
  "for"
  "function"
  "if"
  "in"
  "unset"
  "while"
  "then"
] @keyword

(comment) @comment

(function_definition name: (word) @function)

(file_descriptor) @constant.numeric.integer

[
  (command_substitution)
  (process_substitution)
  (expansion)
] @embedded

[
  "$"
  "&&"
  ">"
  ">>"
  "<"
  "|"
] @operator

(
  (command (_) @constant)
  (#match? @constant "^-")
)
