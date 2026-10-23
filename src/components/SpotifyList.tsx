import type { RefObject } from 'react';
import { describeFailure, isOraError } from '../features/account';
import {
  countLabel,
  isEpisodeList,
  trackTime,
  useEndOfList,
  useWindow,
} from '../features/library';
import type { ListSource, Paged } from '../features/library';
import { releaseDate } from '../features/search';
import type { TrackRow } from '../native/client';
import type { Feature } from '../native/bindings/Feature';
import { SaveButton } from './SaveButton';

type Props = {
  source: Exclude<ListSource, { kind: 'albums' | 'shows' }>;
  list: Paged<TrackRow>;
  /** The `open.spotify.com` page of what's playing now, to mark its row. */
  playingUrl: string | null;
  /** Which tracks (and the album or show itself) are saved; missing while unknown. */
  saved: Map<string, boolean>;
  onPlay: (row: TrackRow) => void;
  onPlayAll: () => void;
  onOpen: (url: string) => void;
  onAllow: (feature: Feature) => void;
  /** Saves `uri` (a track, album or show) or removes it, `saved` being its state now. */
  onSave: (uri: string, saved: boolean) => void;
};

type Heading = {
  eyebrow: string;
  title: string;
  intro: string;
  noun: string;
  empty: string;
  spotifyUrl: string | null;
  /** The album or show, saved as a whole from the heading. */
  container: string | null;
};

function headingFor(source: Props['source']): Heading {
  switch (source.kind) {
    case 'playlist': {
      const { playlist } = source;
      return {
        eyebrow: 'Spotify playlist',
        title: playlist.name || 'Untitled playlist',
        intro: playlist.owner ? `By ${playlist.owner}.` : 'A Spotify playlist.',
        noun: 'track',
        empty: 'This playlist is empty.',
        spotifyUrl: playlist.spotifyUrl,
        container: null,
      };
    }
    case 'album': {
      const { album } = source;
      const year = album.releaseDate.slice(0, 4);
      return {
        eyebrow: 'Spotify album',
        title: album.name || 'Untitled album',
        intro: [album.by.length ? `By ${album.by.join(', ')}` : '', year]
          .filter(Boolean)
          .join(', ')
          .concat('.'),
        noun: 'track',
        empty: 'This album has no tracks.',
        spotifyUrl: album.spotifyUrl,
        container: album.uri,
      };
    }
    case 'show': {
      const { show } = source;
      return {
        eyebrow: 'Spotify podcast',
        title: show.name || 'Untitled podcast',
        intro: show.by.length ? `From ${show.by.join(', ')}.` : 'A podcast.',
        noun: 'episode',
        empty: 'This podcast has no episodes.',
        spotifyUrl: show.spotifyUrl,
        container: show.uri,
      };
    }
    case 'episodes':
      return {
        eyebrow: 'Your Spotify library',
        title: 'Your Episodes',
        intro: 'Podcast episodes you’ve saved on Spotify, newest first.',
        noun: 'episode',
        empty: 'No saved episodes yet. Save one in Spotify to see it here.',
        spotifyUrl: null,
        container: null,
      };
    case 'liked':
      return {
        eyebrow: 'Your Spotify library',
        title: 'Liked Songs',
        intro: 'Songs you’ve liked on Spotify, newest first.',
        noun: 'track',
        empty: 'No Liked Songs yet. Use + beside a song to add some.',
        spotifyUrl: null,
        container: null,
      };
  }
}

/** Loading, a failure, "Show more" or the empty message under a list; it also loads on scroll. */
export function ListFooter<T>({
  list,
  noun,
  empty,
  onAllow,
  end,
}: {
  list: Paged<T>;
  noun: string;
  empty: string;
  onAllow: (feature: Feature) => void;
  end: RefObject<HTMLDivElement | null>;
}) {
  const needs =
    isOraError(list.error) && list.error.kind === 'needsScope'
      ? list.error.feature
      : null;
  return (
    <div ref={end}>
      {list.loading ? (
        <p className="empty-state" role="status">
          Loading from Spotify…
        </p>
      ) : list.error ? (
        <p className="error-message" role="alert">
          {describeFailure(list.error)}{' '}
          {needs ? (
            <button className="link-button" onClick={() => onAllow(needs)}>
              Allow
            </button>
          ) : (
            <button className="link-button" onClick={list.retry}>
              Try again
            </button>
          )}
        </p>
      ) : list.nextOffset !== null ? (
        <p className="empty-state">
          <button className="link-button" onClick={list.loadMore}>
            Show more {noun}s
          </button>
        </p>
      ) : (
        list.items.length === 0 && (
          <p className="empty-state" role="status">
            {empty}
          </p>
        )
      )}
    </div>
  );
}

/** A spacer for rows a long list doesn't render. */
function Spacer({ rows, height }: { rows: number; height: number }) {
  if (rows <= 0) return null;
  return (
    <tr className="spacer-row" aria-hidden="true">
      <td colSpan={4} style={{ height: rows * height }} />
    </tr>
  );
}

/** Liked Songs, a playlist, an album, a podcast or Your Episodes, in the centre's track table. */
export function SpotifyList({
  source,
  list,
  playingUrl,
  saved,
  onPlay,
  onPlayAll,
  onOpen,
  onAllow,
  onSave,
}: Props) {
  const heading = headingFor(source);
  const playlist = source.kind === 'playlist' ? source.playlist : null;
  const episodes = isEpisodeList(source);
  const refused =
    isOraError(list.error) &&
    list.error.kind === 'forbidden' &&
    list.error.hint === 'NotOwnerPlaylist';
  const limited = playlist !== null && (!playlist.readable || refused);
  const total = playlist ? playlist.total : list.total;
  const albumName = source.kind === 'album' ? source.album.name : '';
  // Liked Songs are saved until shown otherwise.
  const isSaved = (uri: string) =>
    saved.get(uri) ??
    (source.kind === 'liked' && uri.startsWith('spotify:track:'));

  const end = useEndOfList(list);
  const [body, rows] = useWindow(list.items.length);
  const shown = list.items.slice(rows.start, rows.end);
  const container = heading.container;

  return (
    <>
      <section className="collection-heading">
        <p className="eyebrow">{heading.eyebrow}</p>
        <h1>{heading.title}</h1>
        <p className="intro">
          {heading.intro} {limited ? '' : 'Select a title to play it.'}
        </p>
        <div className="collection-meta">
          <span>{countLabel(total, heading.noun)}</span>
          <span>From Spotify</span>
          {heading.spotifyUrl && (
            <span>
              <button
                className="link-button"
                onClick={() => onOpen(heading.spotifyUrl!)}
              >
                Open in Spotify
              </button>
            </span>
          )}
          {container && (
            <span>
              <SaveButton
                saved={saved.get(container) === true}
                name={heading.title}
                place="Your Library"
                onToggle={() =>
                  onSave(container, saved.get(container) === true)
                }
              />
              {saved.get(container) ? 'In Your Library' : 'Save'}
            </span>
          )}
        </div>
      </section>
      {limited ? (
        <div className="content-note">
          <span className="note-mark">i</span>
          <p>
            Spotify only shows the tracks of playlists you own or collaborate
            on. You can still play it, or open it in Spotify to see the full
            playlist.{' '}
            <button className="link-button" onClick={onPlayAll}>
              Play playlist
            </button>
          </p>
        </div>
      ) : (
        <>
          <div className="track-table-wrapper">
            <table className="track-table">
              <caption className="sr-only">
                {episodes ? 'Episodes' : 'Tracks'} from Spotify. Select a title
                to play it.
              </caption>
              <thead>
                <tr>
                  <th scope="col">#</th>
                  <th scope="col">{episodes ? 'Episode' : 'Track / artist'}</th>
                  <th scope="col">{episodes ? 'Released' : 'Album'}</th>
                  <th scope="col">Time</th>
                </tr>
              </thead>
              <tbody ref={body}>
                <Spacer rows={rows.start} height={rows.rowHeight} />
                {shown.map((row) => {
                  const playing =
                    row.spotifyUrl !== null && row.spotifyUrl === playingUrl;
                  const track = row.uri?.startsWith('spotify:track:')
                    ? row.uri
                    : null;
                  return (
                    <tr
                      key={row.position}
                      className={playing ? 'selected-track' : ''}
                    >
                      <td className="track-number">
                        {String(row.position + 1).padStart(2, '0')}
                      </td>
                      <td>
                        <button
                          className="track-select"
                          aria-current={playing ? 'true' : undefined}
                          disabled={row.uri === null}
                          title={
                            row.uri === null
                              ? 'Melora can’t play local files or tracks Spotify no longer has.'
                              : undefined
                          }
                          onClick={() => onPlay(row)}
                        >
                          <span>{row.name || 'Unknown title'}</span>
                          <span className="track-artist">
                            {episodes
                              ? row.album || 'Podcast episode'
                              : row.artists.join(', ')}
                          </span>
                        </button>
                      </td>
                      <td className="album-cell">
                        {episodes
                          ? releaseDate(row.releaseDate)
                          : row.album || albumName}
                      </td>
                      <td className="duration">
                        {track && (
                          <SaveButton
                            saved={isSaved(track)}
                            name={row.name}
                            place="Liked Songs"
                            onToggle={() => onSave(track, isSaved(track))}
                          />
                        )}
                        {trackTime(row.durationMs)}
                      </td>
                    </tr>
                  );
                })}
                <Spacer
                  rows={list.items.length - rows.end}
                  height={rows.rowHeight}
                />
              </tbody>
            </table>
          </div>
          <ListFooter
            list={list}
            noun={heading.noun}
            empty={heading.empty}
            onAllow={onAllow}
            end={end}
          />
        </>
      )}
    </>
  );
}
