import type { OraError, SpotifyStatus } from '../native/client';
import { featureUi } from './registry';

export function isOraError(value: unknown): value is OraError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { kind?: unknown }).kind === 'string'
  );
}

/** Copy for an error from the ora plugin, or `null` when nothing should be shown. */
export function describeError(
  error: OraError,
  redirectUri = 'the redirect URI shown in Settings',
): string | null {
  switch (error.kind) {
    case 'cancelled':
      return null;
    case 'signedOut':
      return 'You’re signed out of Spotify.';
    case 'sessionExpired':
      return 'Spotify needs you to sign in again. This happens every six months, or after access is removed.';
    case 'forbidden':
      switch (error.hint) {
        case 'Allowlist':
          return 'Spotify refused access. Check that your Spotify account is listed under User Management for your app in the Spotify Developer Dashboard.';
        case 'Premium':
          return 'Spotify refused: controlling playback needs Spotify Premium.';
        case 'Restricted':
          return 'Spotify doesn’t allow controlling this device from other apps.';
        case 'NotOwnerPlaylist':
          return 'Spotify only shows the tracks of playlists you own or collaborate on. Open it in Spotify to see the full playlist.';
        default:
          return 'Spotify refused this request.';
      }
    case 'rateLimited':
      return 'Spotify is limiting requests. Try again in a moment.';
    case 'quotaExceeded':
      return 'Spotify API quota reached — try again later.';
    case 'offline':
      return 'Spotify can’t be reached. Check your internet connection.';
    case 'network':
      return 'Spotify didn’t respond as expected. Try again.';
    case 'responseTooLarge':
      return 'Spotify sent more than Melora can read at once, so this part of the list can’t be shown.';
    case 'storageUnavailable':
      return 'This device’s credential store isn’t available, so Melora can’t keep you signed in after it closes, or couldn’t remove the saved sign-in.';
    case 'notConfigured':
      return 'Add your Spotify Client ID first.';
    case 'invalidClientId':
      return 'That doesn’t look like a Client ID. Copy it from Basic Information in your app’s settings in the Spotify Developer Dashboard.';
    case 'settingsUnwritable':
      return 'The Client ID couldn’t be read or saved. Check that Melora’s data folder is writable.';
    case 'signIn':
      switch (error.reason) {
        case 'BrowserUnavailable':
          return 'Your web browser couldn’t be opened.';
        case 'TimedOut':
          return `Sign-in timed out. If Spotify showed an error about the redirect URI, add ${redirectUri} exactly to your app’s Redirect URIs, then try again.`;
        case 'InvalidClient':
          return 'Spotify doesn’t recognise this Client ID. Check it in the Spotify Developer Dashboard.';
        case 'InvalidCallback':
          return 'The sign-in didn’t complete. Try again.';
        case 'CallbackPortBusy':
          return 'Another program on this PC is using Melora’s sign-in port. Close it, then try again.';
        case 'ExchangeRejected':
          return `Spotify didn’t accept the sign-in. Check that ${redirectUri} is one of your app’s Redirect URIs, then try again.`;
        default: {
          const unhandled: never = error.reason;
          void unhandled;
        }
      }
      break;
    case 'needsScope':
      return `Spotify needs additional permission for “${featureUi[error.feature].label}”. Allow it, then approve it in your browser.`;
    case 'invalidSpotifyLink':
      return 'That isn’t a Spotify album, artist, playlist, show, track or episode link. In Spotify, use Share → Copy link.';
    case 'notFound':
      return 'Spotify couldn’t find that.';
    case 'noActiveDevice':
      return 'No Spotify device is active. Choose one under Playback device in the player.';
    case 'deviceUnavailable':
      return 'That device is no longer available. Choose another one.';
    case 'unsupported':
      return 'That isn’t available here.';
    default: {
      // A new error kind from ora-types is a type error here until it has copy.
      const unhandled: never = error;
      void unhandled;
    }
  }
  return 'Something went wrong. Try again.';
}

export function describeFailure(
  error: unknown,
  redirectUri?: string,
): string | null {
  if (isOraError(error)) return describeError(error, redirectUri);
  if (error instanceof Error) return error.message;
  return 'Something went wrong. Try again.';
}

/** Signed in, but loading the profile failed; `lastError` says why. */
export function profileFailed(status: SpotifyStatus | null): boolean {
  if (status?.auth.kind !== 'signedIn' || status.auth.profile) return false;
  const kind = status.lastError?.kind;
  // A failed credential-store save doesn't stop the profile from loading.
  return (
    kind !== undefined && kind !== 'cancelled' && kind !== 'storageUnavailable'
  );
}

/**
 * Whether to fetch the missing profile again without being asked: only after a connection
 * problem, never after Spotify refused or limited the request.
 */
export function profileRetryable(status: SpotifyStatus | null): boolean {
  const kind = status?.lastError?.kind;
  return profileFailed(status) && (kind === 'offline' || kind === 'network');
}

export function accountName(status: SpotifyStatus | null): string | null {
  if (status?.auth.kind !== 'signedIn' || !status.auth.profile) return null;
  return status.auth.profile.displayName ?? 'your Spotify account';
}
