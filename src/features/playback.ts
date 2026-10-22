import type { PlaybackStatus, SpotifyStatus } from '../native/client';
import type { PlaybackIssue } from '../native/bindings/PlaybackIssue';

/** The sign-in covers the scopes embedded playback needs. */
export function playbackAllowed(status: SpotifyStatus | null): boolean {
  return (
    status?.auth.kind === 'signedIn' &&
    status.auth.granted.includes('EmbeddedPlayback')
  );
}

export function tierLine(status: PlaybackStatus): string {
  switch (status.tier) {
    case 'off':
      return 'Playback in Melora is off.';
    case 'unsupported':
      return 'Playback in Melora isn’t available on this system: its web engine has no supported DRM. Play in the Spotify app instead.';
    case 'connecting':
      return 'Connecting Melora to Spotify…';
    case 'ready':
      return 'Melora is ready as a Spotify device. Choose Play in Melora, or pick “Melora” in the Spotify app’s device list.';
    case 'active':
      return status.paused ? 'Paused in Melora.' : 'Playing in Melora.';
    default: {
      const unhandled: never = status.tier;
      void unhandled;
      return '';
    }
  }
}

export function issueLine(issue: PlaybackIssue): string {
  const detail = issue.message ? ` Spotify said: “${issue.message}”` : '';
  switch (issue.kind) {
    case 'initialization':
      return `Spotify’s player couldn’t start here.${detail}`;
    case 'authentication':
      return `Spotify didn’t accept the sign-in for playback. Sign out and in again, then allow playback.${detail}`;
    case 'account':
      return `Spotify says this account can’t play here. Playback in other apps needs Spotify Premium.${detail}`;
    case 'playback':
      return `Spotify couldn’t play this item.${detail}`;
    case 'autoplay':
      return 'Audio was blocked until a click. Choose Play in Melora.';
    default: {
      const unhandled: never = issue.kind;
      void unhandled;
      return detail;
    }
  }
}

const yesNo = (value: boolean) => (value ? 'yes' : 'no');

/** One line the owner can read back: no personal data, only engine capabilities. */
export function diagnosticsLine(status: PlaybackStatus): string | null {
  const probe = status.probe;
  if (!probe) return null;
  const parts = [
    `secure context ${yesNo(probe.secure)}`,
    `Widevine ${yesNo(probe.widevine)}`,
    `PlayReady ${yesNo(probe.playready)}`,
    `FairPlay ${yesNo(probe.fairplay)}`,
    `engine ${probe.webviewVersion ?? 'unknown'}`,
    `OS version ${probe.platformVersion ?? 'unknown'}`,
    status.cspBlocks.length === 0
      ? 'CSP blocks none'
      : `CSP blocks ${status.cspBlocks.join('; ')}`,
  ];
  return parts.join(' · ');
}

/** The position now, interpolated from the last report while playing. */
export function positionAt(status: PlaybackStatus, nowMs: number): number {
  if (status.tier !== 'active' || status.paused) return status.positionMs;
  const elapsed = Math.max(0, nowMs - status.updatedAtMs);
  return Math.min(status.durationMs, status.positionMs + elapsed);
}

export function formatTime(ms: number): string {
  const total = Math.floor(ms / 1000);
  const minutes = Math.floor(total / 60);
  const seconds = String(total % 60).padStart(2, '0');
  return `${minutes}:${seconds}`;
}
