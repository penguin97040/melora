import type { OraError, SpotifyStatus } from '../native/client';

export const REDIRECT_URI = 'http://127.0.0.1/callback';

export function isOraError(value: unknown): value is OraError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { kind?: unknown }).kind === 'string'
  );
}

/** Copy for an error from the ora plugin, or `null` when nothing should be shown. */
export function describeError(error: OraError): string | null {
  switch (error.kind) {
    case 'cancelled':
      return null;
    case 'signedOut':
      return 'You’re signed out of Spotify.';
    case 'sessionExpired':
      return 'Spotify needs you to sign in again. This happens every six months, or after access is removed.';
    case 'forbidden':
      return error.hint === 'Allowlist'
        ? 'Spotify refused access. Check that your Spotify account is listed under User Management for your app in the Spotify Developer Dashboard.'
        : 'Spotify refused this request.';
    case 'rateLimited':
      return 'Spotify is limiting requests. Try again in a moment.';
    case 'quotaExceeded':
      return 'Spotify API quota reached — try again later.';
    case 'offline':
      return 'Spotify can’t be reached. Check your internet connection.';
    case 'network':
      return 'Spotify didn’t respond as expected. Try again.';
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
          return `Sign-in timed out. If Spotify showed an error about the redirect URI, add ${REDIRECT_URI} to your app’s Redirect URIs, then try again.`;
        case 'InvalidClient':
          return 'Spotify doesn’t recognise this Client ID. Check it in the Spotify Developer Dashboard.';
        case 'InvalidCallback':
          return 'The sign-in didn’t complete. Try again.';
        case 'ExchangeRejected':
          return `Spotify didn’t accept the sign-in. Check that ${REDIRECT_URI} is one of your app’s Redirect URIs, then try again.`;
        default: {
          const unhandled: never = error.reason;
          void unhandled;
        }
      }
      break;
    case 'needsScope':
      return 'Spotify needs additional permission for this. Sign in again to allow it.';
    case 'notFound':
      return 'Spotify couldn’t find that.';
    case 'noActiveDevice':
      return 'No Spotify device is active.';
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

export function describeFailure(error: unknown): string | null {
  if (isOraError(error)) return describeError(error);
  if (error instanceof Error) return error.message;
  return 'Something went wrong. Try again.';
}

export function accountName(status: SpotifyStatus | null): string | null {
  if (status?.auth.kind !== 'signedIn' || !status.auth.profile) return null;
  return status.auth.profile.displayName ?? 'your Spotify account';
}
