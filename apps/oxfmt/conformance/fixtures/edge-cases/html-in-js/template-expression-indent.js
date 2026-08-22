// An embedded `${expr}` is anchored to the column its line starts at in the
// SOURCE, not the one the embedded printer gave that line. Same rule and same
// reasoning as `css-in-js/template-expression-indent.js`, which carries the
// full note: it matches Prettier 3.9.6, prettier/prettier#19725 drops the
// anchoring on Prettier main, and all three flip together when this fork moves
// to a Prettier carrying that fix.
_ = html`
  <div>
                      ${
                        a + //
                        b
                      }
  </div>
`;

// prettier/prettier#19518: nested embeds were not idempotent
const t = html`
  <ol>
    ${items.map(
      (entry) => html`
        <li>
          ${entry.children
            ? html`
                <ol>
                  ${entry.children.map(
                    (child) => html`<li>${child.title}</li>`,
                  )}
                </ol>
              `
            : entry.title}
        </li>
      `,
    )}
  </ol>
`;

export function foo() {
  return html`
    <div>
              <pre>${JSON.stringify({
                  a: 1,
                  b: 2,
                })}</pre>
    </div>
  `;
}

const a = html`
          ${{
              c: y,
          }}
`;
