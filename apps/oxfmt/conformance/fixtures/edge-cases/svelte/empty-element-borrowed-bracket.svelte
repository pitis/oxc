<!-- An empty inline element renders the whitespace between its tags, so the
     closing tag borrows the opening `>` rather than let a break introduce any.
     The two then move down together, and the attributes stay on the tag's
     line — measuring them stops at that break instead of counting the
     `></span>` that was never going to share the line. -->
<span class="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" title="New"
></span>

<!-- A block element borrows nothing: its `>` follows the attributes directly,
     so the same measurement counts `></div>` and the attribute breaks out. -->
<div
  class="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
></div>

<!-- Both fit outright and stay put. -->
<span class="short" title="New"></span>
<div class="short"></div>
