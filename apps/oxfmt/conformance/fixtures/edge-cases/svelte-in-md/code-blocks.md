# A Svelte component in a fence

```svelte
<script>
    let   count = 0
</script>

<button    onclick={() => count++}   >clicked {count}</button>

<style>
  button{color:red}
</style>
```

Markup only, no script or style:

```svelte
<div     class="a"  >{#if ok}yes{:else}no{/if}</div>
```

Inside a list, where the fence carries an indent:

- an item

  ```svelte
  <p     >text</p>
  ```

Markup the Svelte compiler rejects stays exactly as written:

```svelte
<div>unclosed
```
