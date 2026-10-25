import { granted } from '../features/library';
import type { SpotifyStatus } from '../native/client';
import type { Feature } from '../native/bindings/Feature';

/** The welcome artwork's waveform: bar heights as percentages of its band. */
const WAVE = [
  28, 46, 62, 38, 74, 52, 88, 60, 42, 70, 96, 64, 48, 80, 58, 36, 66, 90, 54,
  40, 72, 50, 32, 44,
];

type Props = {
  view: 'Library' | 'Search';
  spotify: SpotifyStatus | null;
  onOpenSettings: () => void;
  onAllow: (feature: Feature) => void;
};

/** What Library or Search shows before Spotify has anything to show there. */
export function Welcome({ view, spotify, onOpenSettings, onAllow }: Props) {
  const search = view === 'Search';
  const feature: Feature = search ? 'Search' : 'Playlists';
  const kind = spotify?.auth.kind;
  const action =
    kind === 'notConfigured'
      ? { label: 'Set up Spotify', run: onOpenSettings }
      : kind !== 'signedIn'
        ? { label: 'Sign in with Spotify', run: onOpenSettings }
        : !granted(spotify, feature)
          ? {
              label: search ? 'Allow search' : 'Show my Spotify playlists',
              run: () => onAllow(feature),
            }
          : null;
  const intro =
    kind !== 'signedIn'
      ? search
        ? 'Sign in with Spotify to search songs, artists, albums, playlists and podcasts.'
        : 'Sign in with Spotify to see your playlists, albums and podcasts here, and play them on any of your devices.'
      : action
        ? search
          ? 'Melora needs your permission to search Spotify.'
          : 'Melora needs your permission to show your playlists.'
        : 'Choose a playlist, album or podcast in the sidebar.';

  return (
    <section className="welcome" aria-labelledby="welcome-heading">
      <div className="collection-heading">
        <p className="eyebrow">
          {search ? 'Find something to play' : 'Your listening space'}
        </p>
        <h1 id="welcome-heading">{search ? 'Search' : 'Welcome to Melora'}</h1>
        <p className="intro">{intro}</p>
        {action && (
          <div className="account-actions">
            <button className="action-button" onClick={action.run}>
              {action.label}
            </button>
          </div>
        )}
      </div>
      <div className="welcome-art" role="img" aria-label="Melora artwork">
        <div className="welcome-emblem">
          <div className="welcome-orbit" />
          <span className="welcome-mark">m.</span>
        </div>
        <div className="welcome-wave">
          {WAVE.map((height, i) => (
            <span key={i} style={{ height: `${height}%` }} />
          ))}
        </div>
        <span className="cover-caption">MELORA / YOUR LISTENING SPACE</span>
      </div>
    </section>
  );
}
