import type {
  DeviceKind,
  PlaybackStatus,
  SpotifyStatus,
} from '../native/client';
import type { PlaybackIssue } from '../native/bindings/PlaybackIssue';
import { granted } from './library';

/** The sign-in covers the scopes embedded playback needs. */
export function playbackAllowed(status: SpotifyStatus | null): boolean {
  return granted(status, 'EmbeddedPlayback');
}

/** The sign-in covers seeing and controlling the account's Spotify devices. */
export function deviceControlAllowed(status: SpotifyStatus | null): boolean {
  return granted(status, 'ConnectControl');
}

/** The sign-in covers reading and changing the library (the player's + control). */
export function libraryAllowed(status: SpotifyStatus | null): boolean {
  return granted(status, 'LibrarySaveRemove');
}

/** Only tracks get the + for now; Spotify's episode wording isn't settled in Melora. */
export function likeable(spotifyUrl: string | null | undefined): boolean {
  return spotifyUrl?.startsWith('https://open.spotify.com/track/') === true;
}

/** Podcasts get 15-second skips in place of Previous and Next (Spotify's Design Guidelines). */
export function isEpisode(spotifyUrl: string | null | undefined): boolean {
  return spotifyUrl?.startsWith('https://open.spotify.com/episode/') === true;
}

export const SKIP_MS = 15_000;

/** Where a skip of `deltaMs` lands, kept within the item. */
export function skipTarget(
  positionMs: number,
  durationMs: number,
  deltaMs: number,
): number {
  return Math.round(Math.min(durationMs, Math.max(0, positionMs + deltaMs)));
}

/** Something is active: Melora itself, or another device Melora controls. */
export function isLive(status: PlaybackStatus): boolean {
  return status.tier === 'active' || status.device !== null;
}

/** Another device is active, not Melora. */
export function isRemote(status: PlaybackStatus | null): boolean {
  return (
    status !== null &&
    status.tier !== 'active' &&
    status.device !== null &&
    !status.device.thisApp
  );
}

/** The active device's name, as the player and the footer show it. */
export function deviceName(status: PlaybackStatus | null): string {
  if (!status || !isLive(status)) return 'No device connected';
  if (status.tier === 'active' || status.device?.thisApp)
    return 'Melora (this app)';
  return status.device?.name || 'Spotify device';
}

export const deviceKindLabel: Record<DeviceKind, string> = {
  computer: 'Computer',
  smartphone: 'Phone',
  tablet: 'Tablet',
  speaker: 'Speaker',
  tv: 'TV',
  gameConsole: 'Game console',
  automobile: 'Car',
  other: 'Device',
};

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
  if (!isLive(status) || status.paused) return status.positionMs;
  const elapsed = Math.max(0, nowMs - status.updatedAtMs);
  return Math.min(status.durationMs, status.positionMs + elapsed);
}
