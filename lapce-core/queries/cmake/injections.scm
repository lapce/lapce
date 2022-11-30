; src: https://github.com/helix-editor/helix/blob/master/runtime/queries/cmake/injections.scm
; license: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

((line_comment) @injection.content
 (#set! injection.language "comment"))
((bracket_comment) @injection.content
 (#set! injection.language "comment"))