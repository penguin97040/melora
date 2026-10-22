//! Plain data shared by every client and by `ora-core`. Serde only, so it also builds for
//! `wasm32-unknown-unknown`. TypeScript bindings are generated from these types by `cargo test`.

pub mod features;

pub use features::Feature;
