// An embedded `${expr}` is anchored to the column its line starts at IN THE
// SOURCE, not the one the CSS printer gave that line: Prettier dedents the
// expression to the root and rebuilds the source column (`addAlignmentToDoc`),
// so the CSS declaration's own indent does not stack on top of it.
//
// Written this far out, the expression stays this far out for a pass — which is
// why re-formatting this fixture's output moves it again. Prettier 3.9.6 is
// unstable here in the same way, and matching it is the point: every figure in
// this fork is measured against 3.9.6, and a real file (`TanStack/query`'s
// `Devtools.tsx`) needs the anchoring to come back byte-identical. Both sides
// are stable once the source column and the printed one agree, which is every
// input that has been through the formatter once.
//
// prettier/prettier#19725 drops the anchoring on Prettier main. When this fork
// moves to a Prettier carrying that fix, this fixture flips with it.
_ = css`
  a{
    color:
                  ${
                    a
                    // comment
                    + b}
    ;
  }
`;
