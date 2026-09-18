; Adapted from Aider's aider/queries/tree-sitter-languages/bash-tags.scm
; (https://github.com/Aider-AI/aider), licensed Apache-2.0. Every
; `@name.definition.*` and `@name.reference.*` capture is renamed to the
; bare `@name` the tree-sitter-tags crate here requires; the node patterns
; are unchanged. See NOTICE.

(function_definition
  name: (word) @name) @definition.function

(variable_assignment
  name: (variable_name) @name) @definition.variable

(command
  name: (command_name) @name) @reference.call
