; source: https://github.com/virchau13/tree-sitter-astro/blob/22697b0e2413464b7abaea9269c5e83a59e39a83/queries/injections.scm
; license: https://github.com/virchau13/tree-sitter-astro/blob/22697b0e2413464b7abaea9269c5e83a59e39a83/LICENSE
; spdx: MIT

((script_element
  (raw_text) @injection.content)
 (#set! injection.language "javascript"))

((style_element
  (raw_text) @injection.content)
 (#set! injection.language "css"))

((frontmatter
   (raw_text) @injection.content)
 (#set! injection.language "typescript"))

((interpolation
   (raw_text) @injection.content)
 (#set! injection.language "tsx"))