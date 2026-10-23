//! Plain data shared by the UI and `ora-core`. Serde only. TypeScript bindings are generated from
//! these types by `cargo test`.

pub mod auth;
pub mod error;
pub mod features;
pub mod library;
pub mod playback;

pub use auth::{AuthState, Profile, Status};
pub use error::{ForbiddenHint, OraError, SignInFailure};
pub use features::Feature;
pub use library::{PlayRequest, PlaylistPage, PlaylistSummary, TrackPage, TrackRow};
pub use playback::{
    Device, DeviceKind, Disallows, NowPlaying, PlaybackIssue, PlaybackStatus, PlayerCommand, Probe,
    RepeatMode, SdkErrorKind, SdkEvent, SdkState, SdkTrack, Tier,
};
