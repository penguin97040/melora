import type { Feature } from '../native/bindings/Feature';

// How Melora presents each shared feature. `satisfies` makes a new ora feature a type error
// here until Melora wires it or defers it visibly.
export type FeatureUi =
  | { kind: 'wired'; label: string }
  | { kind: 'deferred'; label: string; reason: string };

const phase = (label: string, n: number): FeatureUi => ({
  kind: 'deferred',
  label,
  reason: `Arrives in Phase ${n}`,
});

export const featureUi = {
  // Settings → Spotify account, the sidebar status and the footer account link.
  SignIn: { kind: 'wired', label: 'Sign in with Spotify' },
  Profile: { kind: 'wired', label: 'Profile' },
  OpenInSpotify: { kind: 'wired', label: 'Open in Spotify' },
  // Settings → Playback in Melora (test): the Phase 3 spike, through the shared sdk-bridge.
  // Media keys come from the SDK's Media Session.
  EmbeddedPlayback: { kind: 'wired', label: 'Playback in Melora' },
  MediaKeys: { kind: 'wired', label: 'Media keys' },
  // The right-hand player's Playback device line and its device list (Tier B, Phase 4).
  ConnectControl: { kind: 'wired', label: 'Control another device' },
  DevicePicker: { kind: 'wired', label: 'Device picker' },
  TransferPlayback: { kind: 'wired', label: 'Transfer playback' },
  // The right-hand player, for Melora itself (Tier A) or another device (Tier B). Volume is
  // the Settings slider for Melora and the device list's slider for another device.
  PlayPause: { kind: 'wired', label: 'Play and pause' },
  SkipNextPrevious: { kind: 'wired', label: 'Next and previous' },
  Seek: { kind: 'wired', label: 'Seek' },
  Volume: { kind: 'wired', label: 'Volume' },
  Shuffle: { kind: 'wired', label: 'Shuffle' },
  Repeat: { kind: 'wired', label: 'Repeat' },
  NowPlaying: { kind: 'wired', label: 'Now playing' },
  Queue: phase('Queue', 6),
  AddToQueue: phase('Add to queue', 6),
  // Search, for songs, artists, albums, playlists and episodes (Phase 6b).
  Search: { kind: 'wired', label: 'Search' },
  // The sidebar's Liked Songs and playlists, and their tracks in the centre (Phase 6a).
  SavedTracks: { kind: 'wired', label: 'Liked Songs' },
  SavedAlbums: phase('Saved albums', 6),
  SavedShowsEpisodes: phase('Saved shows and episodes', 6),
  // The player's + (Liked Songs) for the track playing now; library views arrive in Phase 6.
  LibrarySaveRemove: { kind: 'wired', label: 'Save to library' },
  Playlists: { kind: 'wired', label: 'Your playlists' },
  PlaylistDetail: { kind: 'wired', label: 'Playlist tracks' },
  PlaylistEdit: phase('Edit playlists', 6),
  CreatePlaylist: phase('Create playlist', 6),
  RecentlyPlayed: phase('Recently played', 6),
} satisfies Record<Feature, FeatureUi>;
