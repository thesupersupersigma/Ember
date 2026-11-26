//! Built-in plugins.
//!
//! Adding a new plugin:
//!   1. Create `src/plugins/<name>.rs` implementing [`crate::Plugin`].
//!   2. Add `mod <name>;` below.
//!   3. Add `Box::new(<name>::<Type>)` to [`builtins()`].
//! That is the only shared file a new plugin touches.

mod event_observer;
mod silent_typing;

use crate::Plugin;

pub fn builtins() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(silent_typing::SilentTyping),
        Box::new(event_observer::EventObserver),
    ]
}
