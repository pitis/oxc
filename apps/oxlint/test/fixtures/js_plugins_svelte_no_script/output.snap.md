# Exit code
1

# stdout
```
  x whole-file-plugin(report-first-tag): found the first `<div>`
   ,-[files/no_script.svelte:1:1]
 1 | <div>only markup here</div>
   : ^^^^
   `----

  x whole-file-plugin(report-first-tag): found the first `<div>`
   ,-[files/with_script.svelte:4:1]
 3 | </script>
 4 | <div>{a}</div>
   : ^^^^
   `----

Found 0 warnings and 2 errors.
Finished in Xms on 2 files with 1 rules using X threads.
```

# stderr
```
```
