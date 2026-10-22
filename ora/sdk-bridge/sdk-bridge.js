// Shared Web Playback SDK bridge for every ora client. It will load Spotify's SDK, forward its
// events to the `ora` plugin and run playback commands sent back from Rust. The access token is
// fetched on demand through the plugin and never cached here.
//
// Intentionally empty until embedded playback is built.
export {};
