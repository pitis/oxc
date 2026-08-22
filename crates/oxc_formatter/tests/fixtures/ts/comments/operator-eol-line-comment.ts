// An end-of-line line comment right after `=`/`:` keeps its position
// (`= // c` + mandatory break). Known divergence, see AGENTS.md:
// Prettier own-lines it for type aliases, and flushes it past the member and
// its `;` for simple-typed property signatures (`simple: Value; // c`).
//
// A union-valued property signature used to be on that list and no longer is:
// the comment travels with the members, staying beside the operator while they
// do and moving down with them once they take an indent of their own.
//
// `Value` below is NOT indented an extra level: annotation content after a `:`
// break gets no indent of its own — same family as variable/parameter/return
// type annotations, which are Prettier-identical in that shape. The union
// members ARE indented, by the union printer itself.

type Alias = // c
  "VALUE";

type AliasUnion = // c
  | AmemberLongEnoughToMakeTheUnionTypeBreakIntoMultipleLines
  | BmemberLongEnoughToMakeTheUnionTypeBreakIntoMultipleLines;

interface I {
  simple: // c
  Value;
  union: // c
    | AmemberLongEnoughToMakeTheUnionTypeBreakIntoMultipleLines
    | BmemberLongEnoughToMakeTheUnionTypeBreakIntoMultipleLines;
}
