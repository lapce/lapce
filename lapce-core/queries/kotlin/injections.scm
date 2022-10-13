; source: https://github.com/helix-editor/helix/blob/master/runtime/queries/kotlin/injections.scm
; licence: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

((comment) @injection.content
	(#set! injection.language "comment"))

; There are 3 ways to define a regex
;    - "[abc]?".toRegex()
((call_expression
	(navigation_expression
		([(line_string_literal) (multi_line_string_literal)] @injection.content)
		(navigation_suffix
			((simple_identifier) @_function
			(#eq? @_function "toRegex")))))
	(#set! injection.language "regex"))

;    - Regex("[abc]?")
((call_expression
	((simple_identifier) @_function
	(#eq? @_function "Regex"))
	(call_suffix
		(value_arguments
			(value_argument
				[ (line_string_literal) (multi_line_string_literal) ] @injection.content))))
	(#set! injection.language "regex"))

;    - Regex.fromLiteral("[abc]?")
((call_expression
	(navigation_expression
		((simple_identifier) @_class
		(#eq? @_class "Regex"))
		(navigation_suffix
			((simple_identifier) @_function
			(#eq? @_function "fromLiteral"))))
	(call_suffix
		(value_arguments
			(value_argument
				[ (line_string_literal) (multi_line_string_literal) ] @injection.content))))
	(#set! injection.language "regex"))
