//! ember.dll — runs inside Discord Phoenix once injected.
//!
//! v1 job: find the access gate (the check behind "This app isn't available for
//! your account") with ember-sig and patch it so Phoenix opens for any
//! logged-in account. The exact signature is derived in IDA against build
//! 0.1.0.2208 — see prompts/01_find_gate.md.

// TODO(v1): DllMain -> locate gate -> patch.
