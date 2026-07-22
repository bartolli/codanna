# Nix Grammar Analysis

*Generated: 2026-05-21 12:23:48 UTC*

## Statistics
- Total nodes in grammar JSON: 36
- Nodes found in comprehensive.nix: 63
- Nodes handled by parser: 56
- Symbol kinds extracted: 3

## Successfully Handled Nodes
These nodes are in examples and handled by parser:
- !=
- "
- ${
- ''
- (
- )
- *
- +
- .
- /
- ;
- <
- ==
- [
- ]
- apply_expression
- assert
- assert_expression
- attrpath
- attrset_expression
- binary_expression
- binding
- binding_set
- comment
- else
- formal
- formals
- function_expression
- identifier
- if
- if_expression
- in
- indented_string_expression
- inherit
- inherit_from
- integer_expression
- interpolation
- let
- let_expression
- list_expression
- parenthesized_expression
- path_expression
- path_fragment
- rec
- rec_attrset_expression
- select_expression
- source_code
- spath_expression
- string_expression
- string_fragment
- then
- variable_expression
- with
- with_expression
- {
- }

## Implementation Gaps
These nodes appear in comprehensive.nix but aren't handled:
- ,
- :
- =
- ?
- @
- ellipses
- inherited_attrs

## Missing from Examples
These grammar nodes aren't in comprehensive.nix:
- float_expression
- has_attr_expression
- unary_expression

## Symbol Kinds Extracted
- Function
- Parameter
- Variable

