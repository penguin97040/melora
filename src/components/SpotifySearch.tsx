import type { RefObject } from 'react';
import { describeFailure } from '../features/account';
import { countLabel, trackTime, useEndOfList } from '../features/library';
import type { Paged } from '../features/library';
import {
  MAX_QUERY,
  MAX_RESULTS,
  PREVIEW_ROWS,
  releaseDate,
  sectionFor,
  sections,
} from '../features/search';
import type { Searched, SearchSection } from '../features/search';
import type { SearchItem, SearchKind } from '../native/client';

type Props = {
  query: string;
  onQuery: (query: string) => void;
  inputRef: RefObject<HTMLInputElement | null>;
  searched: Searched;
  /** The kind "See all" shows, or `null` for the overview. */
  seeAll: SearchKind | null;
  onSeeAll: (kind: SearchKind | null) => void;
  /** "See all"'s pages. */
  more: Paged<SearchItem>;
  /** The `open.spotify.com` page of what's playing now, to mark its row. */
  playingUrl: string | null;
  onPlay: (kind: SearchKind, items: SearchItem[], item: SearchItem) => void;
};

/** The subtitle and the two cells after the title, per kind. */
function cells(kind: SearchKind, item: SearchItem): [string, string, string] {
  const by = item.by.join(', ');
  const time = item.durationMs === null ? '' : trackTime(item.durationMs);
  const count = item.total === null ? '' : item.total.toLocaleString('en-NZ');
  switch (kind) {
    case 'track':
      return [by, item.album, time];
    case 'artist':
      return ['Artist', 'Top tracks', ''];
    case 'album':
      return [by, item.releaseDate.slice(0, 4), count];
    case 'playlist':
      return [by ? `By ${by}` : 'Spotify playlist', '', count];
    case 'episode':
      return ['Podcast episode', releaseDate(item.releaseDate), time];
  }
}

function ResultTable({
  section,
  items,
  playingUrl,
  onPlay,
}: {
  section: SearchSection;
  items: SearchItem[];
  playingUrl: string | null;
  onPlay: (item: SearchItem) => void;
}) {
  const [title, third, fourth] = section.columns;
  return (
    <div className="track-table-wrapper">
      <table className="track-table">
        <caption className="sr-only">
          {section.title} from Spotify. Select a title to play it.
        </caption>
        <thead>
          <tr>
            <th scope="col">#</th>
            <th scope="col">{title}</th>
            <th scope="col">{third}</th>
            <th scope="col">{fourth}</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const [subtitle, detail, extra] = cells(section.kind, item);
            const playing = item.spotifyUrl === playingUrl;
            return (
              <tr
                key={item.position}
                className={playing ? 'selected-track' : ''}
              >
                <td className="track-number">
                  {String(item.position + 1).padStart(2, '0')}
                </td>
                <td>
                  <button
                    className="track-select"
                    aria-current={playing ? 'true' : undefined}
                    disabled={!item.playable}
                    title={
                      item.playable
                        ? undefined
                        : 'Spotify can’t play this in your country.'
                    }
                    onClick={() => onPlay(item)}
                  >
                    <span>{item.name || 'Untitled'}</span>
                    <span className="track-artist">{subtitle}</span>
                  </button>
                </td>
                <td className="album-cell">{detail}</td>
                <td className="duration">{extra}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** Spotify search in the centre: each kind's top results, or every result of one kind. */
export function SpotifySearch({
  query,
  onQuery,
  inputRef,
  searched,
  seeAll,
  onSeeAll,
  more,
  playingUrl,
  onPlay,
}: Props) {
  const end = useEndOfList(more);
  const wanted = query.trim();
  const section = seeAll ? sectionFor(seeAll) : null;
  const found = searched.results
    ? sections.filter(({ key }) => searched.results![key].items.length > 0)
    : [];

  return (
    <>
      <section className="collection-heading">
        <p className="eyebrow">Search Spotify</p>
        <h1>{section ? section.title : 'Search'}</h1>
        <p className="intro">
          {section
            ? `Spotify’s ${section.noun}s for “${wanted}”. Select a title to play it.`
            : 'Songs, artists, albums, playlists and podcast episodes from Spotify. Select a title to play it.'}
        </p>
        <div className="collection-meta">
          {section && <span>{countLabel(more.total, section.noun)}</span>}
          <span>From Spotify</span>
          {section && (
            <span>
              <button className="link-button" onClick={() => onSeeAll(null)}>
                Back to all results
              </button>
            </span>
          )}
        </div>
      </section>
      <div className="search-field">
        <label htmlFor="search">Search Spotify</label>
        <input
          ref={inputRef}
          id="search"
          type="search"
          value={query}
          maxLength={MAX_QUERY}
          onChange={(e) => onQuery(e.target.value)}
          placeholder="Song, artist, album, playlist or episode…"
        />
      </div>
      {section ? (
        <>
          <ResultTable
            section={section}
            items={more.items}
            playingUrl={playingUrl}
            onPlay={(item) => onPlay(section.kind, more.items, item)}
          />
          <div ref={end}>
            {more.loading ? (
              <p className="empty-state" role="status">
                Loading from Spotify…
              </p>
            ) : more.error ? (
              <p className="error-message" role="alert">
                {describeFailure(more.error)}{' '}
                <button className="link-button" onClick={more.retry}>
                  Try again
                </button>
              </p>
            ) : more.nextOffset !== null ? (
              <p className="empty-state">
                <button className="link-button" onClick={more.loadMore}>
                  Show more {section.noun}s
                </button>
              </p>
            ) : (
              more.total > MAX_RESULTS && (
                <p className="empty-state" role="status">
                  Spotify shows the first {MAX_RESULTS.toLocaleString('en-NZ')}{' '}
                  results. Add words to narrow the search.
                </p>
              )
            )}
          </div>
        </>
      ) : wanted === '' ? (
        <div className="content-note">
          <span className="note-mark">i</span>
          <p>Type to search Spotify. Results appear as you type.</p>
        </div>
      ) : searched.error ? (
        <p className="error-message" role="alert">
          {describeFailure(searched.error)}{' '}
          <button className="link-button" onClick={searched.retry}>
            Try again
          </button>
        </p>
      ) : found.length === 0 ? (
        <p className="empty-state" role="status">
          {searched.loading
            ? 'Searching Spotify…'
            : `Spotify found nothing for “${searched.query}”. Try other words.`}
        </p>
      ) : (
        <div aria-busy={searched.loading}>
          {found.map((item) => {
            const page = searched.results![item.key];
            const shown = page.items.slice(0, PREVIEW_ROWS);
            return (
              <section key={item.kind} aria-label={item.title}>
                <div className="results-heading">
                  <h2 className="eyebrow">{item.title}</h2>
                  {page.total > shown.length && (
                    <button
                      className="link-button"
                      onClick={() => onSeeAll(item.kind)}
                    >
                      See all{' '}
                      {countLabel(Math.min(page.total, MAX_RESULTS), item.noun)}
                    </button>
                  )}
                </div>
                <ResultTable
                  section={item}
                  items={shown}
                  playingUrl={playingUrl}
                  onPlay={(row) => onPlay(item.kind, page.items, row)}
                />
              </section>
            );
          })}
        </div>
      )}
    </>
  );
}
