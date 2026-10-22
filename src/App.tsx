import { useEffect, useRef, useState } from 'react';
import { Player } from './components/Player';
import { SpotifyAccount } from './components/SpotifyAccount';
import { WindowHeader } from './components/WindowHeader';
import { accountName, profileRetryable } from './features/account';
import {
  defaultPreferences,
  getAppInfo,
  getWindowChrome,
  hasNativeStorage,
  loadSpotifyProfile,
  openInSpotify,
  savePreferences,
  showMainWindow,
  subscribeSpotify,
} from './native/client';
import type {
  AppInfo,
  Preferences,
  SpotifyStatus,
  WindowChrome,
} from './native/client';
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
  // Keyed to the URL, so a new profile or account gets a fresh attempt.
  const [failedAvatar, setFailedAvatar] = useState<string | null>(null);
  const revealRequested = useRef(false);
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
                  Preferred volume <small>(for future playback)</small>
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
                onChange={(e) =>
                  updatePreferences({
                    ...preferences,
                    volume: Number(e.target.value),
                  })
                }
              />
              <p className="settings-note">
                {hasNativeStorage()
                  ? 'Preferences are saved on this device. Windows portable builds keep them in the data folder beside Melora.exe.'
                  : 'Browser preview: preferences last for this session. The desktop app saves them on this device.'}
              </p>
              <SpotifyAccount status={spotify} />
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
        <Player track={selected} />
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
          Playback device: <span>No device connected</span>
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
