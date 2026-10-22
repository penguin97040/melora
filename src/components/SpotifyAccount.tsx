import { useState } from 'react';
import {
  cancelSpotifySignIn,
  hasNativeStorage,
  loadSpotifyProfile,
  openInSpotify,
  setSpotifyClientId,
  signInWithSpotify,
  signOutOfSpotify,
} from '../native/client';
import type { SpotifyStatus } from '../native/client';
import {
  accountName,
  describeError,
  describeFailure,
  profileFailed,
} from '../features/account';

function stateLine(status: SpotifyStatus | null): string {
  switch (status?.auth.kind) {
    case undefined:
      return 'Checking the Spotify connection…';
    case 'notConfigured':
      return 'Add a Client ID to sign in.';
    case 'signedOut':
      return 'Not signed in.';
    case 'signingIn':
      return 'Waiting for you to approve Melora in your browser…';
    case 'sessionExpired':
      return 'Your Spotify sign-in has expired. Sign in again.';
    case 'signedIn':
      if (status.auth.profile) return `Signed in as ${accountName(status)}.`;
      return profileFailed(status)
        ? 'Signed in, but your Spotify profile couldn’t be loaded.'
        : 'Signed in. Loading your profile…';
  }
}

export function SpotifyAccount({ status }: { status: SpotifyStatus | null }) {
  const [clientId, setClientId] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const desktop = hasNativeStorage();
  const kind = status?.auth.kind;
  const redirectUri = status?.redirectUri;

  async function run(action: () => Promise<unknown>): Promise<boolean> {
    setBusy(true);
    setMessage(null);
    try {
      await action();
      return true;
    } catch (error) {
      setMessage(describeFailure(error, redirectUri));
      return false;
    } finally {
      setBusy(false);
    }
  }

  // Sign-in waits for the browser for up to five minutes, so it doesn't lock the controls:
  // Cancel must stay available.
  function signIn(): void {
    setMessage(null);
    signInWithSpotify().catch((error: unknown) =>
      setMessage(describeFailure(error, redirectUri)),
    );
  }

  const lastError =
    !message && status?.lastError
      ? describeError(status.lastError, redirectUri)
      : null;
  const profileUrl =
    status?.auth.kind === 'signedIn' ? status.auth.profile?.spotifyUrl : null;

  return (
    <section
      className="account-settings"
      aria-labelledby="spotify-account-heading"
    >
      <h2 id="spotify-account-heading">Spotify account</h2>
      <p className="settings-note">
        Melora connects through your own Spotify app. In the Spotify Developer
        Dashboard, add this exact Redirect URI (it starts with http, not https,
        and includes the port), and add your account under User Management. Then
        paste the app’s Client ID here. Melora never asks for your Spotify
        password or Client Secret.
      </p>
      {redirectUri && (
        <p className="redirect-uri">
          Redirect URI: <code>{redirectUri}</code>
        </p>
      )}
      {status?.clientIdFromEnvironment ? (
        <p className="settings-note">
          The Client ID ({status.clientIdHint}) is set by the development
          environment.
        </p>
      ) : (
        <form
          className="client-id-form"
          onSubmit={(event) => {
            event.preventDefault();
            void run(() => setSpotifyClientId(clientId)).then((saved) => {
              if (saved) setClientId('');
            });
          }}
        >
          <label htmlFor="spotify-client-id">Spotify Client ID</label>
          <div className="client-id-row">
            <input
              id="spotify-client-id"
              value={clientId}
              onChange={(event) => setClientId(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              disabled={!desktop}
              placeholder={
                status?.clientIdHint
                  ? `Saved: ${status.clientIdHint}`
                  : '32 letters and digits'
              }
            />
            <button
              type="submit"
              className="action-button"
              disabled={!desktop || busy || clientId.trim() === ''}
            >
              Save
            </button>
          </div>
        </form>
      )}
      <p className="account-state" role="status">
        {stateLine(status)}
        {status?.auth.kind === 'signedIn' && status.auth.sessionOnly
          ? ' This sign-in lasts until Melora closes, because this device’s credential store isn’t available.'
          : ''}
      </p>
      <div className="account-actions">
        {(kind === 'signedOut' || kind === 'sessionExpired') && (
          <button
            className="action-button"
            disabled={!desktop || busy}
            onClick={signIn}
          >
            Sign in with Spotify
          </button>
        )}
        {kind === 'signingIn' && (
          <button
            className="action-button"
            disabled={busy}
            onClick={() => void run(cancelSpotifySignIn)}
          >
            Cancel sign-in
          </button>
        )}
        {profileFailed(status) && (
          <button
            className="action-button"
            disabled={busy}
            onClick={() => void run(loadSpotifyProfile)}
          >
            Load profile again
          </button>
        )}
        {kind === 'signedIn' && profileUrl && (
          <button
            className="action-button"
            onClick={() => void run(() => openInSpotify(profileUrl))}
          >
            Open in Spotify
          </button>
        )}
        {kind === 'signedIn' && (
          <button
            className="action-button"
            disabled={busy}
            onClick={() => void run(signOutOfSpotify)}
          >
            Sign out
          </button>
        )}
      </div>
      {!desktop && (
        <p className="settings-note">
          Browser preview: Spotify sign-in needs the desktop app.
        </p>
      )}
      {(message ?? lastError) && (
        <p className="error-message account-error" role="alert">
          {message ?? lastError}
        </p>
      )}
    </section>
  );
}
