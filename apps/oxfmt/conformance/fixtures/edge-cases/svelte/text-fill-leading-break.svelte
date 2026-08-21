<!-- A text run carries its own leading break, and that break takes the first
     place in the sequence the printer fills. From there on the breaks fall
     where the words would, so each break is measured on its own — one column
     — instead of against the word that follows it, and the line only ends
     once it is already full. The word that takes it past the width stays
     where it is. -->
<div class="note">
	before.
	<br />
	A run that follows a sibling is laid out this way, so a word that crosses stays put.
</div>

<!-- The leading break of the first child belongs to the element rather than to
     the run, which leaves every word in a place the printer measures against
     the width. That is the ordinary layout, where a word that would cross it
     moves down. -->
<div class="note">
	A run that opens its element is laid out the ordinary way, so a word that crosses moves down.
</div>

<!-- A blank line is two breaks, not one, and the pair puts the words back in
     the places they had — the ordinary layout again. -->
<div class="note">
	before.
	<br />

	A run that follows a blank line is laid out the ordinary way, so a word that crosses moves down.
</div>
