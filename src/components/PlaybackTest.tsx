import { useEffect, useState } from 'react';
import {
  activatePlayback,
  allowPlayback,
  enablePlayback,
  hasNativeStorage,
  openInSpotify,
  playHere,
  sendPlaybackCommand,
} from '../native/client';
import type {
  PlaybackStatus,
  PlayerCommand,
  SpotifyStatus,
} from '../native/client';
import { describeFailure } from '../features/account';
import {
  diagnosticsLine,
  formatTime,
  issueLine,
  playbackAllowed,
  positionAt,
  tierLine,
} from '../features/playback';

const SKIP_MS = 15_000;

/** The current position, ticking locally once a second while playing (no Spotify requests). */
function useInterpolatedPosition(status: PlaybackStatus): number {
  const [now, setNow] = useState(() => Date.now());
  const playing = status.tier === 'active' && !status.paused;
  useEffect(() => {
    // Until the first tick, a stale clock only means no elapsed time (never negative).
    if (!playing) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [playing, status.updatedAtMs]);
  return positionAt(status, now);
}

// Phase 3 spike: a small test block under the Spotify account settings. The approved player
// panel stays a preview until real playback is wired into it (Phase 5).
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
  const [level, setLevel] = useState(volume);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const position = useInterpolatedPosition(playback);
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

  function command(next: PlayerCommand): void {
    void run(() => sendPlaybackCommand(next));
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
        A first test of playing Spotify inside Melora. It needs Spotify Premium.
        The player on the right stays a preview until this works.
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
              onClick={() => void run(() => enablePlayback(level / 100))}
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
        <>
          <p className="account-state">
            {nowPlaying
              ? `${nowPlaying.name}${nowPlaying.artists.length > 0 ? ` — ${nowPlaying.artists.join(', ')}` : ''}${nowPlaying.album ? ` (${nowPlaying.album})` : ''}`
              : 'Nothing is playing.'}{' '}
            · {formatTime(position)} / {formatTime(playback.durationMs)}
          </p>
          <div
            className="account-actions"
            role="group"
            aria-label="Test playback controls"
          >
            <button
              className="action-button"
              onClick={() => command({ op: 'previous' })}
            >
              Previous
            </button>
            <button
              className="action-button"
              onClick={() => command({ op: 'togglePlay' })}
            >
              {playback.paused ? 'Play' : 'Pause'}
            </button>
            <button
              className="action-button"
              onClick={() => command({ op: 'next' })}
            >
              Next
            </button>
            <button
              className="action-button"
              onClick={() =>
                command({
                  op: 'seek',
                  positionMs: Math.max(0, position - SKIP_MS),
                })
              }
            >
              Back 15 s
            </button>
            <button
              className="action-button"
              onClick={() =>
                command({ op: 'seek', positionMs: position + SKIP_MS })
              }
            >
              Forward 15 s
            </button>
            {nowPlaying?.spotifyUrl && (
              <button
                className="action-button"
                onClick={() => {
                  const url = nowPlaying.spotifyUrl;
                  if (url) void run(() => openInSpotify(url));
                }}
              >
                Open in Spotify
              </button>
            )}
          </div>
          <label className="setting-row" htmlFor="playback-volume">
            <span>Test volume</span>
            <span>{level}%</span>
          </label>
          <input
            id="playback-volume"
            type="range"
            min="0"
            max="100"
            value={level}
            onChange={(event) => {
              const next = Number(event.target.value);
              setLevel(next);
              command({ op: 'setVolume', volume: next / 100 });
            }}
          />
        </>
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
