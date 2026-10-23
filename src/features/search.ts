import { useEffect, useState } from 'react';
import { isOraError } from './account';
import { searchPage, searchSpotify } from '../native/client';
import type {
  PlayRequest,
  SearchItem,
  SearchKind,
  SearchResults,
} from '../native/client';

/** Typing settles this long before Melora asks Spotify. */
export const SEARCH_DEBOUNCE_MS = 250;
/** Rust refuses longer queries. */
export const MAX_QUERY = 200;
/** The overview shows this many of each kind; "See all" pages through the rest. */
export const PREVIEW_ROWS = 5;
/** Spotify returns at most this many results of each kind. */
export const MAX_RESULTS = 1000;

export type SearchSection = {
  kind: SearchKind;
  key: keyof SearchResults;
  title: string;
  noun: string;
  /** Column headings after the title's. */
  columns: [string, string, string];
};

export const sections: SearchSection[] = [
  {
    kind: 'track',
    key: 'tracks',
    title: 'Songs',
    noun: 'song',
    columns: ['Track / artist', 'Album', 'Time'],
  },
  {
    kind: 'artist',
    key: 'artists',
    title: 'Artists',
    noun: 'artist',
    columns: ['Artist', 'Plays', ''],
  },
  {
    kind: 'album',
    key: 'albums',
    title: 'Albums',
    noun: 'album',
    columns: ['Album / artist', 'Released', 'Tracks'],
  },
  {
    kind: 'playlist',
    key: 'playlists',
    title: 'Playlists',
    noun: 'playlist',
    columns: ['Playlist / owner', '', 'Tracks'],
  },
  {
    kind: 'episode',
    key: 'episodes',
    title: 'Episodes',
    noun: 'episode',
    columns: ['Episode', 'Released', 'Time'],
  },
];

export function sectionFor(kind: SearchKind): SearchSection {
  return sections.find((section) => section.kind === kind)!;
}

/** Spotify plays at most this many tracks without a context. */
const MAX_ITEMS = 100;

/**
 * Songs play from the clicked one through the songs listed after it; an episode plays alone;
 * artists, albums and playlists play as themselves.
 */
export function searchPlayRequest(
  kind: SearchKind,
  items: SearchItem[],
  item: SearchItem,
): PlayRequest {
  if (kind === 'track') {
    const uris = items
      .slice(items.indexOf(item))
      .filter((row) => row.playable)
      .map((row) => row.uri)
      .slice(0, MAX_ITEMS);
    return { kind: 'items', uris };
  }
  if (kind === 'episode') return { kind: 'items', uris: [item.uri] };
  return { kind: 'context', uri: item.uri, position: null };
}

/** `2021-03-04` → `04/03/2021`; a month or a year alone stays as short. */
export function releaseDate(date: string): string {
  return date.split('-').reverse().join('/');
}

/** The key "See all" pages load under: the kind, then the query. */
export function searchKey(kind: SearchKind, query: string): string {
  return `${kind}:${query}`;
}

export async function fetchSearchPage(key: string, offset: number) {
  const split = key.indexOf(':');
  return searchPage(
    key.slice(split + 1),
    key.slice(0, split) as SearchKind,
    offset,
  );
}

type Done = {
  query: string;
  attempt: number;
  results: SearchResults | null;
  error: unknown;
};

export type Searched = {
  /** What `results` were found for; while a newer query loads, the last results stay. */
  query: string;
  results: SearchResults | null;
  loading: boolean;
  error: unknown;
  retry: () => void;
};

/**
 * Searches Spotify once typing settles. Rust cancels a search a newer one replaces; that
 * `cancelled` is expected and shows nothing.
 */
export function useSearch(query: string, enabled: boolean): Searched {
  const wanted = enabled ? query.trim() : '';
  const [attempt, setAttempt] = useState(0);
  const [done, setDone] = useState<Done | null>(null);

  useEffect(() => {
    if (wanted === '') return;
    let active = true;
    const timer = window.setTimeout(() => {
      void searchSpotify(wanted).then(
        (results) => {
          if (active) setDone({ query: wanted, attempt, results, error: null });
        },
        (error: unknown) => {
          if (!active || (isOraError(error) && error.kind === 'cancelled'))
            return;
          setDone({ query: wanted, attempt, results: null, error });
        },
      );
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [wanted, attempt]);

  const current =
    done?.query === wanted && done.attempt === attempt ? done : null;
  const last = wanted === '' ? null : (current ?? done);
  return {
    query: last?.query ?? '',
    results: last?.results ?? null,
    loading: wanted !== '' && current === null,
    error: current?.error ?? null,
    retry: () => setAttempt((previous) => previous + 1),
  };
}
