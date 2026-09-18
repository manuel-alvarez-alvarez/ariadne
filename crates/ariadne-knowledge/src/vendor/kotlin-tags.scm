; Adapted from Aider's aider/queries/tree-sitter-languages/kotlin-tags.scm
; (https://github.com/Aider-AI/aider), licensed Apache-2.0, written for the
; older tree-sitter-kotlin grammar. `tree-sitter-kotlin-ng` names its nodes
; differently (`identifier` where that grammar had `simple_identifier` and
; `type_identifier`), so the patterns are rewritten for it; the definitions
; and references it names are the same. See NOTICE.

(class_declaration
  name: (identifier) @name) @definition.class

(object_declaration
  name: (identifier) @name) @definition.object

(function_declaration
  name: (identifier) @name) @definition.function

(call_expression
  (identifier) @name) @reference.call

(delegation_specifier
  (constructor_invocation
    (user_type
      (identifier) @name))) @reference.class
