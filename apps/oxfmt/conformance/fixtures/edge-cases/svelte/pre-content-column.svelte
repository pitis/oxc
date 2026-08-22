<!-- A `<pre>` keeps its text as written, tabs and all, and the tags between
     that text are still laid out. So the column an element starts at is the
     one the preserved text left the line on — and a tab there is content, not
     indentation, which is why it is worth nothing. Measuring it as a tab stop
     while printing it as nothing made the printer disagree with itself about
     where the line was, on exactly the lines it cannot re-indent, and elements
     that fit came back broken.

     The first `<span>` below ends one column inside the width and stays whole;
     the second is past it and breaks. -->
<div>
	<div>
		<div>
			<pre class="no-scrollbar max-h-[300px] min-w-0 overflow-x-auto px-4 py-3.5">
				<code data-line-numbers data-language="css" class="-my-10">
					<span data-line class="line text-code-foreground">&nbsp;:root &#123;</span>
					<span data-line class="line text-code-foreground">&nbsp;&nbsp;&nbsp;--radius: 0.65rem;</span>
				</code>
			</pre>
		</div>
	</div>
</div>
