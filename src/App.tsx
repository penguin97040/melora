import { useEffect, useRef, useState } from 'react';
import { DevicePicker } from './components/DevicePicker';
import { PlaybackTest } from './components/PlaybackTest';
import { Player } from './components/Player';
import { SpotifyAccount } from './components/SpotifyAccount';
import { WindowHeader } from './components/WindowHeader';
import { accountName, profileRetryable } from './features/account';
import {
  deviceControlAllowed,
  isRemote,
  libraryAllowed,
  likeable,
  playbackAllowed,
} from './features/playback';
import {
  allowLibrary,
  defaultPreferences,
  enablePlayback,
  getAppInfo,
  getWindowChrome,
  hasNativeStorage,
  loadSpotifyProfile,
  openInSpotify,
  refreshLiked,
  refreshPlayback,
  savePreferences,
  sendPlaybackCommand,
  showMainWindow,
  subscribePlayback,
  subscribeSpotify,
  toggleLiked,
} from './native/client';
import type {
  AppInfo,
  PlaybackStatus,
  PlayerCommand,
  Preferences,
  SpotifyStatus,
  WindowChrome,
} from './native/client';
import { describeFailure } from './features/account';
import { previewPlaylists, previewTracks } from './preview';

function connectionLabel(status: SpotifyStatus | null): string {
  switch (status?.auth.kind) {
    case 'signedIn':
      return 'Signed in to Spotify';
    case 'signingIn':
      return 'Signing in to Spotify…';
    case 'sessionExpired':
      return 'Spotify sign-in expired';
    default:
      return 'Spotify not connected';
  }
}

type View = 'Library' | 'Search' | 'Settings';
const initialTrack = previewTracks[0]!;
/** While Melora isn't playing itself, the active device is read this often (DO_NEXT §4.5). */
export const REMOTE_REFRESH_MS = 30_000;
/** A remote track change shows this long after the track should have ended. */
const TRACK_END_GRACE_MS = 1_500;
/** How long "Added to Liked Songs" replaces the player's mode line. */
export const LIKE_MESSAGE_MS = 3_000;

/** Background reads: a failure here shows nothing and the next trigger tries again. */
function quietly(action: () => Promise<unknown>): void {
  void (async () => {
    try {
      await action();
    } catch {
      // Settings and the device list show errors when the user acts.
    }
  })();
}

export default function App() {
  const [view, setView] = useState<View>('Library');
  const [playlist, setPlaylist] = useState(0);
  const [selected, setSelected] = useState(initialTrack);
  const [query, setQuery] = useState('');
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [preferences, setPreferences] = useState(defaultPreferences);
  const [storageMessage, setStorageMessage] = useState<string | null>(null);
  const [chrome, setChrome] = useState<WindowChrome>('preview');
  const [chromeSettled, setChromeSettled] = useState(false);
  const [windowMessage, setWindowMessage] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [preferencesSettled, setPreferencesSettled] = useState(false);
  const [spotify, setSpotify] = useState<SpotifyStatus | null>(null);
  const [playback, setPlayback] = useState<PlaybackStatus | null>(null);
  const [likeMessage, setLikeMessage] = useState<string | null>(null);
  // Keyed to the URL, so a new profile or account gets a fresh attempt.
  const [failedAvatar, setFailedAvatar] = useState<string | null>(null);
  const revealRequested = useRef(false);
  const launchEnabled = useRef(false);
  const saveQueue = useRef<Promise<unknown>>(Promise.resolve());
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let active = true;
    void getAppInfo()
      .then((result) => {
        if (!active) return;
        setInfo(result);
        setPreferences(result.preferences);
        setStorageMessage(result.storageError);
        setReady(true);
      })
      .catch(() => {
        if (active)
          setStorageMessage(
            'Native application details could not be loaded. Relaunch Melora to try again.',
          );
      })
      .finally(() => {
        if (active) setPreferencesSettled(true);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    void getWindowChrome()
      .then((mode) => {
        if (active) setChrome(mode);
      })
      .catch(() => {
        if (active)
          setWindowMessage(
            'Window controls could not be loaded. Relaunch Melora to try again.',
          );
      })
      .finally(() => {
        if (active) setChromeSettled(true);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    // Status arrives over a channel; it never contains tokens. The subscription lives as long
    // as the page, so the native side stops sending when the page goes away.
    let active = true;
    void subscribeSpotify((status) => {
      if (active) setSpotify(status);
    }).catch(() => {
      if (active)
        setWindowMessage(
          'The Spotify connection status could not be loaded. Relaunch Melora to try again.',
        );
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    void subscribePlayback((status) => {
      if (active) setPlayback(status);
    }).catch(() => {
      if (active)
        setWindowMessage(
          'The playback status could not be loaded. Relaunch Melora to try again.',
        );
    });
    return () => {
      active = false;
    };
  }, []);

  const retryProfile = profileRetryable(spotify);
  useEffect(() => {
    // After a connection problem, try the profile again when the connection or the user comes
    // back. These are events, not polling; a refusal or rate limit is never retried by itself.
    if (!retryProfile) return;
    const retry = () => void loadSpotifyProfile().catch(() => undefined);
    window.addEventListener('online', retry);
    window.addEventListener('focus', retry);
    return () => {
      window.removeEventListener('online', retry);
      window.removeEventListener('focus', retry);
    };
  }, [retryProfile]);

  useEffect(() => {
    document.documentElement.style.fontSize = `${preferences.scale}%`;
    return () => {
      document.documentElement.style.fontSize = '';
    };
  }, [preferences.scale]);

  useEffect(() => {
    // Effects run after the shell commits; the scale effect above has also run.
    // Hidden WebViews need no animation-frame wait to signal readiness.
    if (!preferencesSettled || !chromeSettled || revealRequested.current)
      return;
    revealRequested.current = true;
    void showMainWindow().catch(() => {
      setWindowMessage(
        'The application window could not be shown. Relaunch Melora to try again.',
      );
    });
  }, [preferencesSettled, chromeSettled]);

  useEffect(() => {
    if (view === 'Search') searchRef.current?.focus();
  }, [view]);

  const deviceReady = playback?.tier === 'ready' || playback?.tier === 'active';
  const hereActive = playback?.tier === 'active';
  const remoteActive = isRemote(playback);
  const canControl = deviceControlAllowed(spotify);
  const mayPlayHere = playbackAllowed(spotify);
  const tier = playback?.tier;

  useEffect(() => {
    // Once playback is allowed, Melora becomes a Spotify device at launch, so it's always in
    // the device lists. Problems show in Settings → Playback in Melora.
    if (!mayPlayHere || !preferencesSettled || tier !== 'off') return;
    if (launchEnabled.current) return;
    launchEnabled.current = true;
    quietly(() => enablePlayback(preferences.volume / 100));
  }, [mayPlayHere, preferencesSettled, tier, preferences.volume]);

  useEffect(() => {
    // The active device is read when control becomes possible, on focus, and at a low rate
    // while Melora is visible and not playing itself. Never per second.
    if (!canControl) return;
    const refresh = () => {
      if (document.visibilityState === 'visible') quietly(refreshPlayback);
    };
    refresh();
    window.addEventListener('focus', refresh);
    const timer = hereActive
      ? undefined
      : window.setInterval(refresh, REMOTE_REFRESH_MS);
    return () => {
      window.removeEventListener('focus', refresh);
      if (timer !== undefined) window.clearInterval(timer);
    };
  }, [canControl, hereActive]);

  const remotePlaying =
    remoteActive &&
    playback !== null &&
    !playback.paused &&
    playback.durationMs > 0;
  const positionMs = playback?.positionMs ?? 0;
  const durationMs = playback?.durationMs ?? 0;
  const updatedAtMs = playback?.updatedAtMs ?? 0;
  useEffect(() => {
    // Another device moves to its next track without telling Melora, so read it again just
    // after this one should have ended (and no more often than every 10 s past the end).
    if (!remotePlaying) return;
    const left = durationMs - positionMs - (Date.now() - updatedAtMs);
    const timer = window.setTimeout(
      () => quietly(refreshPlayback),
      left > 0 ? left + TRACK_END_GRACE_MS : 10_000,
    );
    return () => window.clearTimeout(timer);
  }, [remotePlaying, positionMs, durationMs, updatedAtMs]);

  const mayLike = libraryAllowed(spotify);
  const nowUrl = playback?.nowPlaying?.spotifyUrl ?? null;
  const likedKnown = playback?.nowPlaying?.liked != null;
  useEffect(() => {
    // Whether a track is liked is asked once per track, and again on focus in case it was
    // changed in Spotify meanwhile. Rust keeps the answer only while that track plays.
    if (!mayLike || !likeable(nowUrl)) return;
    if (!likedKnown) quietly(refreshLiked);
    const recheck = () => quietly(refreshLiked);
    window.addEventListener('focus', recheck);
    return () => window.removeEventListener('focus', recheck);
  }, [mayLike, nowUrl, likedKnown]);

  useEffect(() => {
    if (likeMessage === null) return;
    const timer = window.setTimeout(
      () => setLikeMessage(null),
      LIKE_MESSAGE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [likeMessage]);

  /** The + control. The first use asks Spotify for library access, then carries on. */
  function toggleLike(): void {
    void (async () => {
      try {
        if (!mayLike && !libraryAllowed(await allowLibrary())) return;
        const liked = await toggleLiked();
        setLikeMessage(
          liked ? 'Added to Liked Songs' : 'Removed from Liked Songs',
        );
      } catch (error) {
        setWindowMessage(describeFailure(error));
      }
    })();
  }

  function playerCommand(command: PlayerCommand): void {
    void sendPlaybackCommand(command).catch((error: unknown) =>
      setWindowMessage(describeFailure(error)),
    );
  }

  /** Melora's own volume: saved as the preference and sent to its player when it's on. */
  function setMeloraVolume(volume: number): void {
    updatePreferences({ ...preferences, volume });
    // While another device plays, commands go to it; Melora's player keeps the preference.
    if (deviceReady && !remoteActive)
      playerCommand({ op: 'setVolume', volume: volume / 100 });
  }

  function openSpotifyPage(url: string): void {
    void openInSpotify(url).catch(() =>
      setWindowMessage('Your browser could not be opened. Try again.'),
    );
  }

  function updatePreferences(next: Preferences): void {
    setPreferences(next);
    // Serialise writes so fast control changes cannot persist stale preferences.
    saveQueue.current = saveQueue.current
      .then(() => savePreferences(next))
      .then(() => {
        setStorageMessage(null);
      })
      .catch(() => {
        setStorageMessage(
          'Preferences cannot be saved. Check that the settings folder is writable. On Windows, move the extracted folder to a writable location.',
        );
      });
  }

  const activePlaylist = previewPlaylists[playlist]!;
  const searchQuery = query.trim().toLocaleLowerCase('en-NZ');
  const tracks = previewTracks.filter((track) =>
    view === 'Search'
      ? `${track.title} ${track.artist} ${track.album}`
          .toLocaleLowerCase('en-NZ')
          .includes(searchQuery)
      : !activePlaylist.artist || track.artist === activePlaylist.artist,
  );
  const artistCount = new Set(tracks.map((track) => track.artist)).size;
  const profile =
    spotify?.auth.kind === 'signedIn' ? spotify.auth.profile : null;

  return (
    <div
      className={`app chrome-${chrome} ${preferences.comfortable ? 'comfortable' : ''}`}
    >
      <a className="skip-link" href="#content">
        Skip to content
      </a>
      <WindowHeader chrome={chrome} onError={setWindowMessage} />
      <div className="workspace">
        <aside className="sidebar panel">
          <nav aria-label="Main navigation">
            <div className="panel-label">
              <span>Explore</span>
            </div>
            {(['Library', 'Search', 'Settings'] as const).map((name, i) => (
              <button
                key={name}
                className={`nav-item ${view === name ? 'active' : ''}`}
                aria-current={view === name ? 'page' : undefined}
                onClick={() => setView(name)}
              >
                <span className="nav-index">0{i + 1}</span>
                {name}
                <span className="nav-arrow">›</span>
              </button>
            ))}
            <p className="connection-note">
              <span
                className={`status-dot ${spotify?.auth.kind === 'signedIn' ? 'connected' : ''}`}
              />
              {spotify?.auth.kind === 'notConfigured' ? (
                <button
                  className="link-button"
                  onClick={() => setView('Settings')}
                >
                  Set up Spotify
                </button>
              ) : (
                connectionLabel(spotify)
              )}
            </p>
          </nav>
          <section className="playlists" aria-label="Preview playlists">
            <div className="panel-label">
              <span>Playlists</span>
              <span>04</span>
            </div>
            <div className="playlist-items">
              {previewPlaylists.map((item, i) => (
                <button
                  key={item.name}
                  className={`playlist-item ${playlist === i ? 'selected' : ''}`}
                  aria-pressed={playlist === i}
                  onClick={() => {
                    setPlaylist(i);
                    setView('Library');
                  }}
                >
                  <span className="playlist-icon">▤</span>
                  <span>
                    <span className="playlist-name">{item.name}</span>
                    <span className="playlist-subtitle">{item.subtitle}</span>
                  </span>
                </button>
              ))}
            </div>
            <p className="sidebar-footnote">
              A local design preview.
              <br />
              No account required.
            </p>
          </section>
        </aside>
        <main id="content" className="content panel" tabIndex={-1}>
          <div className="panel-label">
            <span>{view}</span>
            <span>Local / preview</span>
          </div>
          {windowMessage && (
            <p className="error-message" role="alert">
              {windowMessage}
            </p>
          )}
          {storageMessage && (
            <p className="error-message" role="alert">
              {storageMessage}
            </p>
          )}
          {view === 'Settings' ? (
            <section className="settings">
              <p className="eyebrow">Make room for the music</p>
              <h1>Display & preferences</h1>
              <p className="intro">Adjust the preview to suit your screen.</p>
              <label className="setting-row">
                <span>Interface scale</span>
                <select
                  value={preferences.scale}
                  disabled={!ready}
                  onChange={(e) =>
                    updatePreferences({
                      ...preferences,
                      scale: Number(e.target.value),
                    })
                  }
                >
                  {[100, 125, 150, 175, 200].map((scale) => (
                    <option key={scale} value={scale}>
                      {scale}%
                    </option>
                  ))}
                </select>
              </label>
              <label className="setting-row">
                <span>Comfortable track rows</span>
                <input
                  type="checkbox"
                  checked={preferences.comfortable}
                  disabled={!ready}
                  onChange={(e) =>
                    updatePreferences({
                      ...preferences,
                      comfortable: e.target.checked,
                    })
                  }
                />
              </label>
              <label className="setting-row" htmlFor="volume">
                <span>
                  Volume <small>(Melora’s playback volume)</small>
                </span>
                <span>{preferences.volume}%</span>
              </label>
              <input
                id="volume"
                type="range"
                min="0"
                max="100"
                value={preferences.volume}
                disabled={!ready}
                onChange={(e) => setMeloraVolume(Number(e.target.value))}
              />
              <p className="settings-note">
                {hasNativeStorage()
                  ? 'Preferences are saved on this device. Windows portable builds keep them in the data folder beside Melora.exe.'
                  : 'Browser preview: preferences last for this session. The desktop app saves them on this device.'}
              </p>
              <SpotifyAccount status={spotify} />
              {playback && (
                <PlaybackTest
                  spotify={spotify}
                  playback={playback}
                  volume={preferences.volume}
                />
              )}
            </section>
          ) : (
            <>
              <section className="collection-heading">
                <p className="eyebrow">
                  {view === 'Search'
                    ? 'Find a preview track'
                    : 'Your listening space'}
                </p>
                <h1>{view === 'Search' ? 'Search' : activePlaylist.name}</h1>
                <p className="intro">
                  {view === 'Search'
                    ? 'Search the synthetic titles, artists and albums below.'
                    : 'Invented music. A real space to make it your own.'}
                </p>
                <div className="collection-meta">
                  <span>{tracks.length.toLocaleString('en-NZ')} tracks</span>
                  <span>
                    {artistCount.toLocaleString('en-NZ')} synthetic{' '}
                    {artistCount === 1 ? 'artist' : 'artists'}
                  </span>
                  <span>No audio</span>
                </div>
              </section>
              {view === 'Search' && (
                <div className="search-field">
                  <label htmlFor="search">Search preview tracks</label>
                  <input
                    ref={searchRef}
                    id="search"
                    type="search"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder="Track, artist or album…"
                  />
                </div>
              )}
              <div className="track-table-wrapper">
                <table className="track-table">
                  <caption className="sr-only">
                    Synthetic preview tracks. Select a title to inspect its
                    details.
                  </caption>
                  <thead>
                    <tr>
                      <th scope="col">#</th>
                      <th scope="col">Track / artist</th>
                      <th scope="col">Album</th>
                      <th scope="col">Time</th>
                    </tr>
                  </thead>
                  <tbody>
                    {tracks.map((track) => (
                      <tr
                        key={track.id}
                        className={
                          selected.id === track.id ? 'selected-track' : ''
                        }
                      >
                        <td className="track-number">
                          {String(track.id).padStart(2, '0')}
                        </td>
                        <td>
                          <button
                            className="track-select"
                            aria-pressed={selected.id === track.id}
                            onClick={() => setSelected(track)}
                          >
                            <span>{track.title}</span>
                            <span className="track-artist">{track.artist}</span>
                          </button>
                        </td>
                        <td className="album-cell">{track.album}</td>
                        <td className="duration">{track.duration}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {tracks.length === 0 && (
                <p className="empty-state" role="status">
                  No preview tracks match “{query}”. Try another title or
                  artist.
                </p>
              )}
              <div className="content-note">
                <span className="note-mark">i</span>
                <p>
                  This is a synthetic preview. Spotify sign-in, your library and
                  playback are coming in later milestones.
                </p>
              </div>
            </>
          )}
        </main>
        <Player
          track={selected}
          playback={playback}
          devicePicker={
            <DevicePicker
              spotify={spotify}
              playback={playback}
              volume={preferences.volume}
              onVolume={setMeloraVolume}
              onCommand={playerCommand}
              onOpenSettings={() => setView('Settings')}
            />
          }
          likeMessage={likeMessage}
          onCommand={playerCommand}
          onToggleLike={toggleLike}
          onOpenInSpotify={openSpotifyPage}
        />
      </div>
      <footer className="status-strip">
        <span>
          <span className="status-dot" />
          Synthetic preview
        </span>
        {profile && (
          <span className="account-chip">
            {profile.imageUrl && profile.imageUrl !== failedAvatar && (
              <img
                className="avatar"
                src={profile.imageUrl}
                alt=""
                width={16}
                height={16}
                loading="lazy"
                referrerPolicy="no-referrer"
                onError={() => setFailedAvatar(profile.imageUrl)}
              />
            )}
            <span>Spotify: {accountName(spotify)}</span>
            {profile.spotifyUrl && (
              <button
                className="link-button"
                onClick={() => {
                  const url = profile.spotifyUrl;
                  if (url)
                    void openInSpotify(url).catch(() =>
                      setWindowMessage(
                        'Your browser could not be opened. Try again.',
                      ),
                    );
                }}
              >
                Open in Spotify
              </button>
            )}
          </span>
        )}
        <span className="playback-device">
          Playback device:{' '}
          <span>
            {hereActive || playback?.device?.thisApp
              ? 'This app'
              : remoteActive
                ? playback?.device?.name || 'Spotify device'
                : 'No device connected'}
          </span>
        </span>
        <span className="stream-quality">Stream quality: unavailable</span>
        <span role="status">
          {info
            ? `v${info.version} / ${info.platform}`
            : 'Native details pending'}
        </span>
      </footer>
    </div>
  );
}
