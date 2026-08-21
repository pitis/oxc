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

<!-- Hugging is the test, not inlineness: a component is neither an inline nor
     a block element, and it borrows. A `<textarea>` hugs too, but it renders
     its own whitespace, so its opening tag carries no break for the attributes
     to dedent to and they break on their own. -->
<Skeleton class="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
></Skeleton>
<textarea rows="4" placeholder="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" id="value"
></textarea>

<!-- An element that hugs neither side borrows nothing: its content is
     whitespace, which it renders, so the tags keep a break between them. -->
<Skeleton class="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"> </Skeleton>

<!-- Both fit outright and stay put. -->
<span class="short" title="New"></span>
<div class="short"></div>
<Skeleton class="short"></Skeleton>
