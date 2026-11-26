//! Built-in plugins.
//!
//! Adding a new plugin:
//!   1. Create `src/plugins/<name>.rs` implementing [`crate::Plugin`].
//!   2. Add `mod <name>;` below.
//!   3. Add `Box::new(<name>::<Type>)` to [`builtins()`].
//! That is the only shared file a new plugin touches.

mod clear_urls;
mod event_observer;
mod send_timestamps;
mod silent_typing;
mod text_replace;
mod unindent;

use crate::Plugin;

pub fn builtins() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(clear_urls::ClearUrls),
        Box::new(event_observer::EventObserver),
        Box::new(send_timestamps::SendTimestamps),
        Box::new(silent_typing::SilentTyping),
        Box::new(text_replace::TextReplace),
        Box::new(unindent::Unindent),
    ]
}
