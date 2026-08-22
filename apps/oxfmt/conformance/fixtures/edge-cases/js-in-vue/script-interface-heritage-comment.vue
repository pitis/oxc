<script lang="ts">
// A comment between an interface's name and its `extends` keyword makes the
// clause breakable — the name keeps its line and `extends …` moves under it.
// A comment between the keyword and the heritage type is a different place: it
// leads the type, the clause stays on the name's line, and the type arguments
// are what break. Telling the two apart is the whole rule; what says which side
// a comment is on is whether everything from the name to it is whitespace.
export interface AfterKeyword extends /** @vue-ignore */ Omit<ImgHTMLAttrs, "src" | "alt"> {
	a: string;
}
export interface OnTheName /** @vue-ignore */ extends Omit<ImgHTMLAttrs, "src"> {
	a: string;
}
export interface NoComment extends Omit<ImgHTMLAttributesLongerXx, "src" | "alt"> {
	a: string;
}
export interface TwoClauses extends AaaaaaaaaaaaaaaaaaaaaaaaLong, BbbbbbbbbbbbbbbbbbbbLong {
	a: string;
}
</script>

<template>
	<img />
</template>
