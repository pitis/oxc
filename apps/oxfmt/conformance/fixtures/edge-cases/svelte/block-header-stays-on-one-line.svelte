<script lang="ts">
	let components: Record<string, { initialData: { configuration: unknown } }> = {};
	let items: { name: string }[] = [];
	let promise = Promise.resolve(1);
	let longlonglonglongName = { anotherLongProperty: { andAnotherOne: 'value' } };
	let customisationByComponent: { components: string[]; link?: string }[] = [];
	let type = 'button';
</script>

<!-- A block header is one line however long it gets: the expression that
     decides what a `{#each}` iterates reads as part of the marker, not as
     content laid out beside it. Calls and member chains are the cases that
     used to break anyway, because they print through a `BestFitting` whose
     variants a shallow flattening pass never reached. -->
{#each Object.entries(components['buttoncomponent'].initialData.configuration) as [key, initialConfig] (key)}
	<span>{key}{initialConfig}</span>
{/each}

{#each items.filter((item) => item.name === longlonglonglongName.anotherLongProperty.andAnotherOne) as item}
	<span>{item.name}</span>
{/each}

{#if longlonglonglongName.anotherLongProperty.andAnotherOne && longlonglonglongName.anotherLongProperty.andAnotherOne}
	<span>yes</span>
{/if}

{#await promise.then((value) => value + 1).catch(() => 0) then resolvedValueWithALongName}
	<span>{resolvedValueWithALongName}</span>
{/await}

{#key longlonglonglongName.anotherLongProperty.andAnotherOne + longlonglonglongName.anotherLongProperty.andAnotherOne}
	<span>keyed</span>
{/key}

<!-- Flattening a call that does not fit reaches the variant with every
     argument on a line of its own, whose breaks are spaces once flattened —
     so the parentheses come back with the room the broken form would have
     used. Prettier's own flattening lands there too. -->
{#each customisationByComponent.filter((c) => c.components.includes(type)) as customisation (customisation.components.join('-'))}
	<span>{customisation.link}</span>
{/each}
