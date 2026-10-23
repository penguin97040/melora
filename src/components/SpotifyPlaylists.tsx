import { describeFailure } from '../features/account';
import { countLabel, sourceKey } from '../features/library';
import type { ListSource, Paged } from '../features/library';
import type { PlaylistSummary } from '../native/client';

type Props = {
  playlists: Paged<PlaylistSummary>;
  /** The key of the list shown in the centre, if it's one of these. */
  selected: string | null;
  onSelect: (source: ListSource) => void;
};

/** The library's own lists, above the playlists. */
const LIBRARY: {
  source: ListSource;
  icon: string;
  name: string;
  subtitle: string;
}[] = [
  {
    source: { kind: 'liked' },
    icon: '♡',
    name: 'Liked Songs',
    subtitle: 'Your saved tracks',
  },
  {
    source: { kind: 'albums' },
    icon: '◎',
    name: 'Albums',
    subtitle: 'Your saved albums',
  },
  {
    source: { kind: 'shows' },
    icon: '◉',
    name: 'Podcasts',
    subtitle: 'Podcasts you follow',
  },
  {
    source: { kind: 'episodes' },
    icon: '▷',
    name: 'Your Episodes',
    subtitle: 'Your saved episodes',
  },
  {
    source: { kind: 'recent' },
    icon: '↺',
    name: 'Recently Played',
    subtitle: 'Your last 50 tracks',
  },
  {
    source: { kind: 'queue' },
    icon: '≡',
    name: 'Queue',
    subtitle: 'What plays next',
  },
];

/**
 * The sidebar's library while signed in: Liked Songs, the saved lists, Recently Played and the
 * queue, then playlists.
 */
export function SpotifyPlaylists({ playlists, selected, onSelect }: Props) {
  const count = playlists.total + LIBRARY.length;
  return (
    <section className="playlists" aria-label="Your Spotify playlists">
      <div className="panel-label">
        <span>Playlists</span>
        <span>{String(count).padStart(2, '0')}</span>
      </div>
      <div className="playlist-items">
        {LIBRARY.map(({ source, icon, name, subtitle }) => {
          const active = selected === sourceKey(source);
          return (
            <button
              key={name}
              className={`playlist-item ${active ? 'selected' : ''}`}
              aria-pressed={active}
              onClick={() => onSelect(source)}
            >
              <span className="playlist-icon">{icon}</span>
              <span>
                <span className="playlist-name">{name}</span>
                <span className="playlist-subtitle">{subtitle}</span>
              </span>
            </button>
          );
        })}
        {playlists.items.map((playlist) => {
          const source: ListSource = { kind: 'playlist', playlist };
          const active = selected === sourceKey(source);
          return (
            <button
              key={playlist.id}
              className={`playlist-item ${active ? 'selected' : ''}`}
              aria-pressed={active}
              onClick={() => onSelect(source)}
            >
              <span className="playlist-icon">▤</span>
              <span>
                <span className="playlist-name">
                  {playlist.name || 'Untitled playlist'}
                </span>
                <span className="playlist-subtitle">
                  {[playlist.owner, countLabel(playlist.total)]
                    .filter(Boolean)
                    .join(' · ')}
                </span>
              </span>
            </button>
          );
        })}
      </div>
      {playlists.loading ? (
        <p className="sidebar-footnote" role="status">
          Loading your playlists…
        </p>
      ) : playlists.error ? (
        <p className="sidebar-footnote" role="alert">
          {describeFailure(playlists.error)}{' '}
          <button className="link-button" onClick={playlists.retry}>
            Try again
          </button>
        </p>
      ) : (
        <p className="sidebar-footnote">
          {playlists.nextOffset !== null && (
            <>
              <button className="link-button" onClick={playlists.loadMore}>
                Show more playlists
              </button>
              <br />
            </>
          )}
          From your Spotify library.
        </p>
      )}
    </section>
  );
}
