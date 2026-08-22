// An embedded `${expr}` is anchored to the column its line starts at in the
// SOURCE, not the one the embedded printer gave that line. Same rule and same
// reasoning as `css-in-js/template-expression-indent.js`, which carries the
// full note: it matches Prettier 3.9.6, prettier/prettier#19725 drops the
// anchoring on Prettier main, and all three flip together when this fork moves
// to a Prettier carrying that fix.
_ = gql`

                  ${
                    a
                    // comment
                    + b}

`;
