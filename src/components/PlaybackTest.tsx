import { useState } from 'react';
import {
  activatePlayback,
  allowPlayback,
  enablePlayback,
  hasNativeStorage,
  playHere,
} from '../native/client';
import type { PlaybackStatus, SpotifyStatus } from '../native/client';
import { describeFailure } from '../features/account';
import {
  diagnosticsLine,
  formatTime,
  issueLine,
  playbackAllowed,
  tierLine,
} from '../features/playback';
import { usePlaybackPosition } from '../features/usePlaybackPosition';

// Phase 3: switching embedded playback on, under the Spotify account settings. While Melora
// plays, the song and its controls are in the right-hand player; this block keeps the setup
// steps and the diagnostics the owner reads back.
export function PlaybackTest({
  spotify,
  playback,
  volume,
}: {
  spotify: SpotifyStatus | null;
  playback: PlaybackStatus;
  volume: number;
}) {
  const [link, setLink] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const position = usePlaybackPosition(playback);
  const desktop = hasNativeStorage();
  const signedIn = spotify?.auth.kind === 'signedIn';
  const allowed = playbackAllowed(spotify);
  const { tier } = playback;
  const device = tier === 'ready' || tier === 'active';

  async function run(action: () => Promise<unknown>): Promise<void> {
    setBusy(true);
    setMessage(null);
    try {
      await action();
    } catch (error) {
      setMessage(describeFailure(error));
    } finally {
      setBusy(false);
    }
  }

  // Approval happens in the browser and can take minutes, so it doesn't lock the controls.
  function allow(): void {
    setMessage(null);
    allowPlayback().catch((error: unknown) =>
      setMessage(describeFailure(error)),
    );
  }

  function play(): void {
    // The SDK needs this inside the click itself, before anything is awaited.
    activatePlayback();
    void run(() => playHere(link));
  }

  const nowPlaying = playback.nowPlaying;
  const diagnostics = diagnosticsLine(playback);
  const issue = playback.issue ? issueLine(playback.issue) : null;

  return (
    <section
      className="account-settings playback-test"
      aria-labelledby="playback-test-heading"
    >
      <h2 id="playback-test-heading">Playback in Melora (test)</h2>
      <p className="settings-note">
        Plays Spotify inside Melora. It needs Spotify Premium. While Melora is
        playing, the song and its controls appear in the player on the right,
        and the Volume setting above controls it.
      </p>
      <p className="account-state" role="status">
        {!signedIn
          ? 'Sign in to Spotify first.'
          : !allowed
            ? 'Spotify needs your permission before Melora can play music.'
            : tierLine(playback)}
      </p>
      {signedIn && (
        <div className="account-actions">
          {!allowed && (
            <button
              className="action-button"
              disabled={!desktop}
              onClick={allow}
            >
              Allow playback
            </button>
          )}
          {allowed && (tier === 'off' || tier === 'unsupported') && (
            <button
              className="action-button"
              disabled={!desktop || busy}
              onClick={() => void run(() => enablePlayback(volume / 100))}
            >
              Enable playback in Melora
            </button>
          )}
        </div>
      )}
      {allowed && device && (
        <form
          className="client-id-form"
          onSubmit={(event) => {
            event.preventDefault();
            play();
          }}
        >
          <label htmlFor="playback-link">
            Spotify link (optional: Share → Copy link in Spotify)
          </label>
          <div className="client-id-row">
            <input
              id="playback-link"
              value={link}
              onChange={(event) => setLink(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              placeholder="Empty plays what you last played"
            />
            <button type="submit" className="action-button" disabled={busy}>
              Play in Melora
            </button>
          </div>
        </form>
      )}
      {tier === 'active' && (
        <p className="account-state">
          {nowPlaying
            ? `${nowPlaying.name}${nowPlaying.artists.length > 0 ? ` — ${nowPlaying.artists.join(', ')}` : ''}${nowPlaying.album ? ` (${nowPlaying.album})` : ''}`
            : 'Nothing is playing.'}{' '}
          · {formatTime(position)} / {formatTime(playback.durationMs)}
        </p>
      )}
      {diagnostics && (
        <p className="redirect-uri">
          Diagnostics: <code>{diagnostics}</code>
        </p>
      )}
      {(message ?? issue) && (
        <p className="error-message account-error" role="alert">
          {message ?? issue}
        </p>
      )}
    </section>
  );
}
