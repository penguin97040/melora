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
  EmbeddedPlayback: phase('Playback in Melora', 3),
  MediaKeys: phase('Media keys', 3),
  ConnectControl: phase('Control another device', 4),
  DevicePicker: phase('Device picker', 4),
  TransferPlayback: phase('Transfer playback', 4),
  PlayPause: phase('Play and pause', 5),
  SkipNextPrevious: phase('Next and previous', 5),
  Seek: phase('Seek', 5),
  Volume: phase('Volume', 5),
  Shuffle: phase('Shuffle', 5),
  Repeat: phase('Repeat', 5),
  NowPlaying: phase('Now playing', 5),
  Queue: phase('Queue', 6),
  AddToQueue: phase('Add to queue', 6),
  Search: phase('Search', 6),
  SavedTracks: phase('Liked Songs', 6),
  SavedAlbums: phase('Saved albums', 6),
  SavedShowsEpisodes: phase('Saved shows and episodes', 6),
  LibrarySaveRemove: phase('Save to library', 6),
  Playlists: phase('Playlists', 6),
  PlaylistDetail: phase('Playlist detail', 6),
  PlaylistEdit: phase('Edit playlists', 6),
  CreatePlaylist: phase('Create playlist', 6),
  RecentlyPlayed: phase('Recently played', 6),
} satisfies Record<Feature, FeatureUi>;
