<script lang="ts">
  // A lone, constraint-less type parameter on an arrow function keeps its
  // trailing comma here, though JSX cannot appear in a `lang="ts"` script:
  // Prettier decides that comma from the file's extension, and `.svelte` is
  // not `.ts`. In a real `.ts` file the same line loses the comma.
  const identity = <T,>(value: T) => value;
  const withDefault = <T = unknown,>(value: T) => value;

  // A constraint disambiguates on its own, so this one has no comma to keep.
  const constrained = <T extends object>(value: T) => value;
</script>

<p>{identity(1)}{withDefault(2)}{constrained({})}</p>
