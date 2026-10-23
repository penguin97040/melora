import { useEffect, useRef } from 'react';
import { describeFailure, isOraError } from '../features/account';
import { countLabel, trackTime } from '../features/library';
import type { ListSource, Paged } from '../features/library';
import type { TrackRow } from '../native/client';
import type { Feature } from '../native/bindings/Feature';

type Props = {
  source: ListSource;
  list: Paged<TrackRow>;
  /** The `open.spotify.com` page of what's playing now, to mark its row. */
  playingUrl: string | null;
  onPlay: (row: TrackRow) => void;
  onPlayAll: () => void;
  onOpen: (url: string) => void;
  onAllow: (feature: Feature) => void;
};

/** Liked Songs or a playlist from Spotify, in the centre's track table. */
export function SpotifyList({
  source,
  list,
  playingUrl,
  onPlay,
  onPlayAll,
  onOpen,
  onAllow,
}: Props) {
  const playlist = source.kind === 'playlist' ? source.playlist : null;
  const refused =
    isOraError(list.error) &&
    list.error.kind === 'forbidden' &&
    list.error.hint === 'NotOwnerPlaylist';
  const limited = playlist !== null && (!playlist.readable || refused);
  const total = playlist ? playlist.total : list.total;
  const needs =
    isOraError(list.error) && list.error.kind === 'needsScope'
      ? list.error.feature
      : null;

  const more = useRef<HTMLDivElement>(null);
  const { nextOffset, loading, loadMore } = list;
  useEffect(() => {
    // The next page loads as the end of the list scrolls into view; the button does the same.
    const end = more.current;
    if (!end || nextOffset === null || loading) return;
    if (typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) loadMore();
    });
    observer.observe(end);
    return () => observer.disconnect();
  }, [nextOffset, loading, loadMore]);

  return (
    <>
      <section className="collection-heading">
        <p className="eyebrow">
          {playlist ? 'Spotify playlist' : 'Your Spotify library'}
        </p>
        <h1>
          {playlist ? playlist.name || 'Untitled playlist' : 'Liked Songs'}
        </h1>
        <p className="intro">
          {playlist
            ? playlist.owner
              ? `By ${playlist.owner}.`
              : 'A Spotify playlist.'
            : 'Songs you’ve liked on Spotify, newest first.'}{' '}
          {limited ? '' : 'Select a title to play it.'}
        </p>
        <div className="collection-meta">
          <span>{countLabel(total)}</span>
          <span>From Spotify</span>
          {playlist && (
            <span>
              <button
                className="link-button"
                onClick={() => onOpen(playlist.spotifyUrl)}
              >
                Open in Spotify
              </button>
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
                Tracks from Spotify. Select a title to play it.
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
                {list.items.map((row) => {
                  const playing =
                    row.spotifyUrl !== null && row.spotifyUrl === playingUrl;
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
                            {row.artists.join(', ')}
                          </span>
                        </button>
                      </td>
                      <td className="album-cell">{row.album}</td>
                      <td className="duration">{trackTime(row.durationMs)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div ref={more}>
            {list.loading ? (
              <p className="empty-state" role="status">
                Loading from Spotify…
              </p>
            ) : list.error ? (
              <p className="error-message" role="alert">
                {describeFailure(list.error)}{' '}
                {needs ? (
                  <button
                    className="link-button"
                    onClick={() => onAllow(needs)}
                  >
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
                  Show more tracks
                </button>
              </p>
            ) : (
              list.items.length === 0 && (
                <p className="empty-state" role="status">
                  {playlist
                    ? 'This playlist is empty.'
                    : 'No Liked Songs yet. Use + in the player to add some.'}
                </p>
              )
            )}
          </div>
        </>
      )}
    </>
  );
}
