; source: https://github.com/helix-editor/helix/blob/master/runtime/queries/protobuf/highlights.scm
; licence: https://github.com/helix-editor/helix/blob/master/LICENSE
; spdx: MPL-2.0

[
  "syntax"
  "package"
  "option"
  "import"
  "service"
  "rpc"
  "returns"
  "message"
  "enum"
  "oneof"
  "repeated"
  "reserved"
  "to"
  "stream"
  "extend"
  "optional"
] @keyword

[
  (keyType)
  (type)
] @type.builtin

[
  (mapName)
  (enumName)
  (messageName)
  (extendName)
  (serviceName)
  (rpcName)
] @type

[
  (fieldName)
  (optionName)
] @variable.other.member
(enumVariantName) @type.enum.variant

(fullIdent) @namespace

(intLit) @constant.numeric.integer
(floatLit) @constant.numeric.float
(boolLit) @constant.builtin.boolean
(strLit) @string

(constant) @constant

(comment) @comment

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
]  @punctuation.bracket