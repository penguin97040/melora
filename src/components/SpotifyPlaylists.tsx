import { useRef } from 'react';
import { describeFailure } from '../features/account';
import { moveKey } from '../features/editing';
import { countLabel, sourceKey } from '../features/library';
import type { ListSource, Paged } from '../features/library';
import { menuPoint } from '../features/menu';
import type { MenuPoint } from '../features/menu';
import { useDragReorder } from '../features/reorder';
import type { PlaylistSummary } from '../native/client';
import { NewPlaylist } from './NewPlaylist';

type Props = {
  /** In the order shown: Melora's own, or Spotify's. */
  playlists: Paged<PlaylistSummary>;
  /** The key of the list shown in the centre, if it's one of these. */
  selected: string | null;
  onSelect: (source: ListSource) => void;
  /** Whether Melora's own order is in use (rather than Spotify's). */
  ordered: boolean;
  /** Moves the playlist shown at `from` to `to` in Melora's order. */
  onMove: (from: number, to: number) => void;
  onUseSpotifyOrder: () => void;
  /** A playlist's context menu (right-click, the Menu key or Shift+F10). */
  onMenu: (playlist: PlaylistSummary, index: number, at: MenuPoint) => void;
  /** The New playlist form: closed (`null`), or open with the song waiting to go in. */
  creating: { adding: string | null; busy: boolean } | null;
  onStartCreate: () => void;
  onCreate: (name: string) => void;
  onCancelCreate: () => void;
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
 * queue, then playlists. Playlists can be put in Melora's own order by dragging them, Alt+↑ or
 * Alt+↓, or their menu; Spotify's order stays as it is.
 */
export function SpotifyPlaylists({
  playlists,
  selected,
  onSelect,
  ordered,
  onMove,
  onUseSpotifyOrder,
  onMenu,
  creating,
  onStartCreate,
  onCreate,
  onCancelCreate,
}: Props) {
  const count = playlists.total + LIBRARY.length;
  const list = useRef<HTMLDivElement>(null);
  const reorder = useDragReorder(list, onMove);
  const last = playlists.items.length - 1;

  return (
    <section className="playlists" aria-label="Your Spotify playlists">
      <div className="panel-label">
        <span>Playlists</span>
        <span>
          {creating === null && (
            <button
              className="panel-action"
              aria-label="New playlist"
              onClick={onStartCreate}
            >
              + New
            </button>
          )}
          {String(count).padStart(2, '0')}
        </span>
      </div>
      {creating && (
        <NewPlaylist
          adding={creating.adding}
          busy={creating.busy}
          onCreate={onCreate}
          onCancel={onCancelCreate}
        />
      )}
      <div
        ref={list}
        className={`playlist-items ${reorder.drag ? 'reordering' : ''}`}
      >
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
        {playlists.items.map((playlist, i) => {
          const source: ListSource = { kind: 'playlist', playlist };
          const active = selected === sourceKey(source);
          return (
            <button
              key={playlist.id}
              {...reorder.bind(i)}
              className={`playlist-item ${active ? 'selected' : ''} ${reorder.classFor(i)}`}
              aria-pressed={active}
              aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
              onClick={() => onSelect(source)}
              onKeyDown={(event) => {
                const step = moveKey(event);
                if (step === null) return;
                event.preventDefault();
                const to = i + step;
                if (to >= 0 && to <= last) onMove(i, to);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                onMenu(playlist, i, menuPoint(event));
              }}
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
          {ordered ? (
            <>
              In Melora’s order.{' '}
              <button className="link-button" onClick={onUseSpotifyOrder}>
                Use Spotify’s order
              </button>
            </>
          ) : (
            'From your Spotify library.'
          )}
          <br />
          Drag a playlist, or press Alt+↑ or Alt+↓, to move it.
        </p>
      )}
    </section>
  );
}
