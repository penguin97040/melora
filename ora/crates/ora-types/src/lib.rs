//! Plain data shared by every client and by `ora-core`. Serde only, so it also builds for
//! `wasm32-unknown-unknown`. TypeScript bindings are generated from these types by `cargo test`.

pub mod auth;
pub mod error;
pub mod features;
pub mod playback;

pub use auth::{AuthState, Profile, Status};
pub use error::{ForbiddenHint, OraError, SignInFailure};
pub use features::Feature;
pub use playback::{
    Disallows, NowPlaying, PlaybackIssue, PlaybackStatus, PlayerCommand, Probe, RepeatMode,
    SdkErrorKind, SdkEvent, SdkState, SdkTrack, Tier,
};
