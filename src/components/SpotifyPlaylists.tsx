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

/** The sidebar's playlists while signed in: Liked Songs, then the account's own list. */
export function SpotifyPlaylists({ playlists, selected, onSelect }: Props) {
  const liked: ListSource = { kind: 'liked' };
  const count = playlists.total + 1;
  return (
    <section className="playlists" aria-label="Your Spotify playlists">
      <div className="panel-label">
        <span>Playlists</span>
        <span>{String(count).padStart(2, '0')}</span>
      </div>
      <div className="playlist-items">
        <button
          className={`playlist-item ${selected === sourceKey(liked) ? 'selected' : ''}`}
          aria-pressed={selected === sourceKey(liked)}
          onClick={() => onSelect(liked)}
        >
          <span className="playlist-icon">♡</span>
          <span>
            <span className="playlist-name">Liked Songs</span>
            <span className="playlist-subtitle">Your saved tracks</span>
          </span>
        </button>
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
