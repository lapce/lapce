; src: https://github.com/helix-editor/helix/blob/master/runtime/queries/clojure/injections.scm
; license: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

((regex_lit) @injection.content
 (#set! injection.language "regex"))
 