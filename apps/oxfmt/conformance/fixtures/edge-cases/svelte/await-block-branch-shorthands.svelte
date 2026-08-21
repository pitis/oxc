<!-- `then` and `catch` decide which branch the body belongs to, with or
     without a binding. Dropping a bindingless `then` is not a formatting
     difference: `{#await p then}` renders its body once the promise resolves,
     while `{#await p}` renders it while the promise is unsettled. -->
{#await promise then}
	<p>resolved, binds nothing</p>
{/await}

{#await promise then value}
	<p>resolved as {value}</p>
{/await}

{#await promise catch}
	<p>rejected, binds nothing</p>
{/await}

{#await promise catch error}
	<p>rejected as {error}</p>
{/await}

{#await promise}
	<p>still pending</p>
{/await}

{#await promise}
	<p>pending</p>
{:then value}
	<p>resolved as {value}</p>
{:catch error}
	<p>rejected as {error}</p>
{/await}
