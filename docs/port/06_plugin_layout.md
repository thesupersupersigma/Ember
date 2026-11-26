# Plugin layout

`crates/ember-plugin/` uses a one-file-per-plugin layout so multiple
plugins can be developed in parallel without merge conflicts.

## Directory structure

```
crates/ember-plugin/src/
  lib.rs              # public API, hook registry, ABI mirror, install()
  plugins/
    mod.rs            # builtins() registry — the only shared file
    silent_typing.rs  # drops POST /channels/*/typing
```

## Adding a new plugin

1. Create `src/plugins/<name>.rs` implementing `crate::Plugin`.
2. In `src/plugins/mod.rs`, add `mod <name>;` and one
   `Box::new(<name>::<Type>)` line to `builtins()`.

That is the only shared file a new plugin touches.
