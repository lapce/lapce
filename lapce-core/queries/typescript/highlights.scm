; source: https://github.com/helix-editor/helix/blob/master/runtime/queries/typescript/highlights.scm
; licence: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

; Namespaces

(internal_module
  [((identifier) @namespace) ((nested_identifier (identifier) @namespace))])

(ambient_declaration "global" @namespace)


; Variables

(required_parameter (identifier) @variable.parameter)
(optional_parameter (identifier) @variable.parameter)

; Punctuation

[
  ":"
] @punctuation.delimiter

(optional_parameter "?" @punctuation.special)
(property_signature "?" @punctuation.special)

(conditional_type ["?" ":"] @operator)



; Keywords

[
  "abstract"
  "declare"
  "export"
  "infer"
  "implements"
  "keyof"
  "namespace"
] @keyword

[
  "type"
  "interface"
  "enum"
] @keyword.storage.type

[
  "public"
  "private"
  "protected"
  "readonly"
] @keyword.storage.modifier

; Special identifiers
;--------------------

([
    (identifier)
    (shorthand_property_identifier)
    (shorthand_property_identifier_pattern)
 ] @constant
 (#match? @constant "^[A-Z_][A-Z\\d_]+$"))


((identifier) @constructor
 (#match? @constructor "^[A-Z]"))

((identifier) @variable.builtin
 (#match? @variable.builtin "^(arguments|module|console|window|document)$")
 (#is-not? local))

((identifier) @function.builtin
 (#eq? @function.builtin "require")
 (#is-not? local))

; Function and method definitions
;--------------------------------

(function
  name: (identifier) @function)
(function_declaration
  name: (identifier) @function)
(method_definition
  name: (property_identifier) @function.method)

(pair
  key: (property_identifier) @function.method
  value: [(function) (arrow_function)])

(assignment_expression
  left: (member_expression
    property: (property_identifier) @function.method)
  right: [(function) (arrow_function)])

(variable_declarator
  name: (identifier) @function
  value: [(function) (arrow_function)])

(assignment_expression
  left: (identifier) @function
  right: [(function) (arrow_function)])
  

; Function and method calls
;--------------------------

(call_expression
  function: (identifier) @function)

(call_expression
  function: (member_expression
    property: (property_identifier) @function.method))

; Variables
;----------

(identifier) @variable

; Properties
;-----------

(property_identifier) @variable.other.member
(shorthand_property_identifier) @variable.other.member
(shorthand_property_identifier_pattern) @variable.other.member

; Literals
;---------

(this) @variable.builtin
(super) @variable.builtin

[
  (true)
  (false)
  (null)
  (undefined)
] @constant.builtin

(comment) @comment

[
  (string)
  (template_string)
] @string

(regex) @string.regexp
(number) @constant.numeric.integer

; Tokens
;-------

(template_substitution
  "${" @punctuation.special
  "}" @punctuation.special) @embedded

[
  ";"
  "?."
  "."
  ","
] @punctuation.delimiter

[
  "-"
  "--"
  "-="
  "+"
  "++"
  "+="
  "*"
  "*="
  "**"
  "**="
  "/"
  "/="
  "%"
  "%="
  "<"
  "<="
  "<<"
  "<<="
  "="
  "=="
  "==="
  "!"
  "!="
  "!=="
  "=>"
  ">"
  ">="
  ">>"
  ">>="
  ">>>"
  ">>>="
  "~"
  "^"
  "&"
  "|"
  "^="
  "&="
  "|="
  "&&"
  "||"
  "??"
  "&&="
  "||="
  "??="
  "..."
] @operator

(ternary_expression ["?" ":"] @operator)

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
]  @punctuation.bracket

[
  "as"
  "async"
  "debugger"
  "delete"
  "extends"
  "function"
  "get"
  "in"
  "instanceof"
  "new"
  "of"
  "set"
  "static"
  "target"
  "try"
  "typeof"
  "void"
  "with"
] @keyword

[
  "class"
  "let"
  "const"
  "var"
] @keyword.storage.type

[
  "switch"
  "case"
  "if"
  "else"
  "yield"
  "throw"
  "finally"
  "return"
  "catch"
  "continue"
  "while"
  "break"
  "for"
  "do"
  "await"
] @keyword.control

[
  "import"
  "default"
  "from"
  "export"
] @keyword.control.import 

; Types

(type_identifier) @type
(predefined_type) @type.builtin

(type_arguments
  "<" @punctuation.bracket
  ">" @punctuation.bracket)

((identifier) @type
 (#match? @type "^[A-Z]"))