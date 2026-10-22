import type { PlaybackStatus } from './client';

/** A playback status for tests: off, with nothing playing, unless overridden. */
export function playbackStatus(
  extra: Partial<PlaybackStatus> = {},
): PlaybackStatus {
  return {
    tier: 'off',
    nowPlaying: null,
    paused: true,
    positionMs: 0,
    durationMs: 0,
    updatedAtMs: 0,
    shuffle: false,
    repeat: 'off',
    disallows: {
      pause: false,
      resume: false,
      next: false,
      previous: false,
      seek: false,
    },
    issue: null,
    probe: null,
    cspBlocks: [],
    ...extra,
  };
}
