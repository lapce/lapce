; From nvim-treesitter/nvim-treesitter

(fenced_code_block
  (info_string
    (language) @injection.language)
  (code_fence_content) @injection.content (#set! injection.include-unnamed-children))

((html_block) @injection.content (#set! injection.language "html") (#set! injection.include-unnamed-children))

([
  (minus_metadata)
  (plus_metadata)
] @injection.content (#set! injection.language "yaml") (#set! injection.include-unnamed-children))

((inline) @injection.content (#set! injection.language "markdown.inline") (#set! injection.include-unnamed-children))