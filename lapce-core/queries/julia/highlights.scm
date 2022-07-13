(identifier) @variable

(operator) @operator
(range_expression ":" @operator)
(pair_expression "=>" @operator)

;; In case you want type highlighting based on Julia naming conventions (this might collide with mathematical notation)
;((identifier) @type ; exception: mark `A_foo` sort of identifiers as variables
  ;(match? @type "^[A-Z][^_]"))
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z_]{2}[A-Z_]*$"))

(macro_identifier) @function.macro
(macro_identifier (identifier) @function.macro) ; for any one using the variable highlight
(macro_definition
  name: (identifier) @function.macro
  ["macro" "end" @keyword])

(field_expression
  (identifier)
  (identifier) @field .)

(function_definition
  name: (identifier) @function)
(call_expression
  (identifier) @function)
(call_expression
  (field_expression (identifier) @method .))
(broadcast_call_expression
  (identifier) @function)
(broadcast_call_expression
  (field_expression (identifier) @method .))
(parameter_list
  (identifier) @parameter)
(parameter_list
  (optional_parameter .
    (identifier) @parameter))
(typed_parameter
  (identifier) @parameter
  (identifier) @type)
(type_parameter_list
  (identifier) @type)
(typed_parameter
  (identifier) @parameter
  (parameterized_identifier) @type)
(function_expression
  . (identifier) @parameter)
(spread_parameter) @parameter
(spread_parameter
  (identifier) @parameter)
(named_argument
    . (identifier) @parameter)
(argument_list
  (typed_expression
    (identifier) @parameter
    (identifier) @type))
(argument_list
  (typed_expression
    (identifier) @parameter
    (parameterized_identifier) @type))

;; Symbol expressions (:my-wanna-be-lisp-keyword)
(quote_expression
 (identifier)) @symbol

;; Parsing error! foo (::Type) gets parsed as two quote expressions
(argument_list
  (quote_expression
    (quote_expression
      (identifier) @type)))

(type_argument_list
  (identifier) @type)
(parameterized_identifier (_)) @type
(argument_list
  (typed_expression . (identifier) @parameter))

(typed_expression
  (identifier) @type .)
(typed_expression
  (parameterized_identifier) @type .)

(abstract_definition
  name: (identifier) @type)
(struct_definition
  name: (identifier) @type)

(subscript_expression
  (_)
  (range_expression
    (identifier) @constant.builtin .)
  (#eq? @constant.builtin "end"))

"end" @keyword

(if_statement
  ["if" "end"] @conditional)
(elseif_clause
  ["elseif"] @conditional)
(else_clause
  ["else"] @conditional)
(ternary_expression
  ["?" ":"] @conditional)

(function_definition ["function" "end"] @keyword.function)

[
  "abstract"
  "const"
  "macro"
  "primitive"
  "struct"
  "type"
] @keyword

"return" @keyword.return

((identifier) @keyword (#any-of? @keyword "global" "local"))

(compound_expression
  ["begin" "end"] @keyword)
(try_statement
  ["try" "end" ] @exception)
(finally_clause
  "finally" @exception)
(catch_clause
  "catch" @exception)
(quote_statement
  ["quote" "end"] @keyword)
(let_statement
  ["let" "end"] @keyword)
(for_statement
  ["for" "end"] @repeat)
(while_statement
  ["while" "end"] @repeat)
(break_statement) @repeat
(continue_statement) @repeat
(for_clause
  "for" @repeat)
(do_clause
  ["do" "end"] @keyword)

"in" @keyword.operator

(export_statement
  ["export"] @include)

(import_statement
  ["import" "using"] @include)

(module_definition
  ["module" "end"] @include)

((identifier) @include (#eq? @include "baremodule"))


;;; Literals

(integer_literal) @number
(float_literal) @float

((identifier) @float
  (#any-of? @float "NaN" "NaN16" "NaN32"
                   "Inf" "Inf16" "Inf32"))

((identifier) @boolean
  (#any-of? @boolean "true" "false"))

((identifier) @constant.builtin
  (#any-of? @constant.builtin "nothing" "missing"))

(character_literal) @character
(escape_sequence) @string.escape

(string_literal) @string
(prefixed_string_literal
  prefix: (identifier) @function.macro) @string

(command_literal) @string.special
(prefixed_command_literal
  prefix: (identifier) @function.macro) @string.special

[
  (line_comment)
  (block_comment)
] @comment

;;; Punctuation

(quote_expression ":" @symbol)
["::" "." "," "..."] @punctuation.delimiter
["[" "]" "(" ")" "{" "}"] @punctuation.bracket
