import { useEffect, useState } from 'react';
import type { PlaybackStatus } from '../native/client';
import { positionAt } from './playback';

/** The current position, ticking locally once a second while playing (no Spotify requests). */
export function usePlaybackPosition(status: PlaybackStatus | null): number {
  const [now, setNow] = useState(() => Date.now());
  const playing = status?.tier === 'active' && !status.paused;
  const reported = status?.updatedAtMs;
  useEffect(() => {
    // Until the first tick, a stale clock only means no elapsed time (never negative).
    if (!playing) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [playing, reported]);
  return status ? positionAt(status, now) : 0;
}
