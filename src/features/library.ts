import { useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import {
  albumTracks,
  getQueue,
  likedTracks,
  listPlaylists,
  playlistTracks,
  recentlyPlayed,
  savedAlbums,
  savedEpisodes,
  savedShows,
  savedStates,
  showEpisodes,
} from '../native/client';
import type {
  CatalogItem,
  PlayRequest,
  PlaylistSummary,
  QueueView,
  SpotifyStatus,
  TrackRow,
} from '../native/client';
import type { Feature } from '../native/bindings/Feature';

/** What the sidebar (or a search result) can show in the centre. */
export type ListSource =
  | { kind: 'liked' }
  | { kind: 'playlist'; playlist: PlaylistSummary }
  | { kind: 'episodes' }
  | { kind: 'album'; album: CatalogItem }
  | { kind: 'show'; show: CatalogItem }
  /** Saved albums or saved shows: a list of albums or shows to open. */
  | { kind: 'albums' }
  | { kind: 'shows' }
  /** The latest tracks played, newest first. */
  | { kind: 'recent' }
  /** What plays now and next; Spotify doesn't let apps reorder it. */
  | { kind: 'queue' };

/** Sources listed as tracks and episodes in the centre's track table. */
export type TrackSource = Exclude<
  ListSource,
  { kind: 'albums' | 'shows' | 'queue' }
>;

export function isTrackList(source: ListSource): source is TrackSource {
  return !isCatalog(source) && source.kind !== 'queue';
}

/** Sources whose rows are albums or shows rather than tracks and episodes. */
export type CatalogSource = Extract<ListSource, { kind: 'albums' | 'shows' }>;

export function isCatalog(source: ListSource): source is CatalogSource {
  return source.kind === 'albums' || source.kind === 'shows';
}

/** Lists of episodes, which play one at a time. */
export function isEpisodeList(source: ListSource): boolean {
  return source.kind === 'episodes' || source.kind === 'show';
}

export function granted(status: SpotifyStatus | null, feature: Feature) {
  return (
    status?.auth.kind === 'signedIn' && status.auth.granted.includes(feature)
  );
}

/** The ID in `spotify:<kind>:<id>`. */
export function idOf(uri: string): string {
  return uri.slice(uri.lastIndexOf(':') + 1);
}

/** The key a list's pages are loaded under; Rust caches them too. */
export function sourceKey(source: ListSource): string {
  switch (source.kind) {
    case 'playlist':
      return `playlist:${source.playlist.id}`;
    case 'album':
      return `album:${idOf(source.album.uri)}`;
    case 'show':
      return `show:${idOf(source.show.uri)}`;
    default:
      return source.kind;
  }
}

/** Spotify plays at most this many tracks from a list without a context. */
const MAX_ITEMS = 100;

/**
 * A playlist or album plays from the clicked row; an episode plays alone; Liked Songs plays the
 * loaded rows from there.
 */
export function playRequestFor(
  source: ListSource,
  rows: TrackRow[],
  row: TrackRow,
): PlayRequest {
  if (source.kind === 'playlist')
    return {
      kind: 'context',
      uri: `spotify:playlist:${source.playlist.id}`,
      position: row.position,
    };
  if (source.kind === 'album')
    return { kind: 'context', uri: source.album.uri, position: row.position };
  if (isEpisodeList(source) && row.uri)
    return { kind: 'items', uris: [row.uri] };
  const uris = rows
    .slice(rows.indexOf(row))
    .flatMap((item) => (item.uri ? [item.uri] : []))
    .slice(0, MAX_ITEMS);
  return { kind: 'items', uris };
}

export function countLabel(total: number, kind = 'track'): string {
  return `${total.toLocaleString('en-NZ')} ${kind}${total === 1 ? '' : 's'}`;
}

/** `m:ss`, or `h:mm:ss` for long episodes. */
export function trackTime(ms: number): string {
  const total = Math.floor(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, '0');
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}`
    : `${minutes}:${seconds}`;
}

export type Page<T> = {
  items: T[];
  total: number;
  nextOffset: number | null;
};

export async function fetchPlaylists(_key: string, offset: number) {
  return listPlaylists(offset);
}

export async function fetchTracks(key: string, offset: number) {
  const split = key.indexOf(':');
  const id = key.slice(split + 1);
  switch (split < 0 ? key : key.slice(0, split)) {
    case 'liked':
      return likedTracks(offset);
    case 'episodes':
      return savedEpisodes(offset);
    case 'album':
      return albumTracks(id, offset);
    case 'show':
      return showEpisodes(id, offset);
    case 'recent':
      return recentlyPlayed();
    default:
      return playlistTracks(id, offset);
  }
}

export async function fetchCatalog(key: string, offset: number) {
  // Home keys its lists per account (`albums:<account>`).
  return key.split(':')[0] === 'albums'
    ? savedAlbums(offset)
    : savedShows(offset);
}

export type Queue = {
  view: QueueView | null;
  loading: boolean;
  error: unknown;
  refresh: () => void;
};

/**
 * The queue for `scope` (the account; `null` reads nothing), read again whenever `trigger`
 * changes (the item playing now) or on `refresh`. The last answer stays while a new one loads.
 */
export function useQueue(scope: string | null, trigger: string): Queue {
  const [refreshes, setRefreshes] = useState(0);
  const [state, setState] = useState<{
    scope: string | null;
    want: string;
    view: QueueView | null;
    error: unknown;
  }>({ scope: null, want: '', view: null, error: null });
  const want = `${trigger}#${refreshes}`;

  useEffect(() => {
    if (scope === null) return;
    let active = true;
    void getQueue().then(
      (view) => {
        if (active) setState({ scope, want, view, error: null });
      },
      (error: unknown) => {
        if (!active) return;
        setState((previous) => ({
          scope,
          want,
          view: previous.scope === scope ? previous.view : null,
          error,
        }));
      },
    );
    return () => {
      active = false;
    };
  }, [scope, want]);

  const current = state.scope === scope && scope !== null;
  const settled = current && state.want === want;
  return {
    view: current ? state.view : null,
    loading: scope !== null && !settled,
    error: settled ? state.error : null,
    refresh: () => setRefreshes((n) => n + 1),
  };
}

/** `savedStates` takes at most this many URIs at once. */
const MAX_STATES = 500;

export type SavedStates = {
  /** Whether each URI asked about is saved; missing while unknown. */
  known: Map<string, boolean>;
  remember: (uris: string[], saved: boolean) => void;
};

/**
 * Whether these tracks, albums or shows are saved, asked once per list (`scope`) for the URIs
 * not known yet. Rust asks Spotify 40 at a time and remembers answers for a minute. A failure
 * leaves them unknown, which shows as not saved.
 */
export function useSavedStates(
  scope: string | null,
  uris: string[],
): SavedStates {
  const [state, setState] = useState({
    scope,
    known: new Map<string, boolean>(),
  });
  if (state.scope !== scope) setState({ scope, known: new Map() });

  const known =
    state.scope === scope ? state.known : new Map<string, boolean>();
  const missing =
    scope === null ? '' : uris.filter((uri) => !known.has(uri)).join(',');
  useEffect(() => {
    if (missing === '') return;
    let active = true;
    const ask = missing.split(',').slice(0, MAX_STATES);
    void savedStates(ask).then(
      (saved) => {
        if (!active) return;
        setState((previous) => {
          if (previous.scope !== scope) return previous;
          const next = new Map(previous.known);
          ask.forEach((uri, i) => next.set(uri, saved[i] === true));
          return { scope, known: next };
        });
      },
      () => undefined,
    );
    return () => {
      active = false;
    };
  }, [scope, missing]);

  return {
    known,
    remember: (changed, saved) =>
      setState((previous) => {
        const next = new Map(previous.known);
        changed.forEach((uri) => next.set(uri, saved));
        return { ...previous, known: next };
      }),
  };
}

type Want = { offset: number; attempt: number };

type Loaded<T> = Page<T> & {
  key: string | null;
  /** The last offset loaded, or -1. */
  loaded: number;
  /** The request that last arrived: a reload asks for offset 0 again, with a new attempt. */
  done: Want | null;
  error: unknown;
  failed: Want | null;
};

const nothing = {
  key: null,
  items: [],
  total: 0,
  nextOffset: null,
  loaded: -1,
  done: null,
  error: null,
  failed: null,
};

const same = (a: Want | null, b: Want) =>
  a?.offset === b.offset && a.attempt === b.attempt;

export type Paged<T> = Page<T> & {
  loading: boolean;
  error: unknown;
  loadMore: () => void;
  retry: () => void;
  /** Changes the loaded rows in place, after an edit Spotify has accepted or will. */
  edit: (change: (page: Page<T>) => Page<T>) => void;
  /** Loads the list again from its first page, keeping the rows shown until it arrives. */
  reload: () => void;
};

/**
 * Pages of a Spotify list, loaded one at a time as they're asked for. A new `key` starts again;
 * `null` loads nothing. `fetchPage` must be a stable function. `account` keeps one account's
 * rows from ever showing for another under the same key; it isn't passed to `fetchPage`.
 */
export function usePaged<T>(
  key: string | null,
  fetchPage: (key: string, offset: number) => Promise<Page<T>>,
  account = '',
): Paged<T> {
  const identity = key === null ? null : `${account}\n${key}`;
  const [want, setWant] = useState({ key: identity, offset: 0, attempt: 0 });
  const [data, setData] = useState<Loaded<T>>(nothing);
  if (want.key !== identity) setWant({ key: identity, offset: 0, attempt: 0 });

  const { offset, attempt } = want;
  const wantKey = want.key;
  useEffect(() => {
    if (wantKey === null) return;
    let active = true;
    const asked = { offset, attempt };
    void fetchPage(wantKey.slice(wantKey.indexOf('\n') + 1), offset).then(
      (page) => {
        if (!active) return;
        setData((previous) => {
          const more = previous.key === wantKey && offset > 0;
          if (more && previous.loaded >= offset)
            return { ...previous, done: asked };
          return {
            key: wantKey,
            items: more ? [...previous.items, ...page.items] : page.items,
            total: page.total,
            nextOffset: page.nextOffset,
            loaded: offset,
            done: asked,
            error: null,
            failed: null,
          };
        });
      },
      (error: unknown) => {
        if (!active) return;
        setData((previous) => ({
          ...(previous.key === wantKey ? previous : nothing),
          key: wantKey,
          error,
          failed: asked,
        }));
      },
    );
    return () => {
      active = false;
    };
  }, [wantKey, offset, attempt, fetchPage]);

  const current: Loaded<T> = data.key === identity ? data : nothing;
  const failedNow = same(current.failed, want);
  // Loading until the page asked for arrives: a new list straight away, and a reload too, so
  // nothing loads more from the old rows meanwhile.
  const loading = identity !== null && !same(current.done, want) && !failedNow;
  return {
    items: current.items,
    total: current.total,
    nextOffset: current.nextOffset,
    loading,
    error: failedNow ? current.error : null,
    loadMore: () => {
      const next = current.nextOffset;
      if (next === null || loading) return;
      // Asking for the page already asked for changes nothing and keeps its attempt, so a
      // failed page waits for Try again (`retry`) and nothing re-renders in a loop.
      setWant((previous) =>
        previous.key === identity && previous.offset === next
          ? previous
          : { key: identity, offset: next, attempt: 0 },
      );
    },
    retry: () =>
      setWant((previous) => ({ ...previous, attempt: previous.attempt + 1 })),
    edit: (change) =>
      setData((previous) => {
        if (previous.key !== identity) return previous;
        const { items, total, nextOffset } = change(previous);
        return { ...previous, items, total, nextOffset };
      }),
    reload: () =>
      setWant((previous) => ({
        key: previous.key,
        offset: 0,
        attempt: previous.attempt + 1,
      })),
  };
}

/**
 * The next page loads as the element this ref is attached to (the end of the list) scrolls
 * into view; a button beside it does the same.
 */
export function useEndOfList<T>(list: Paged<T>) {
  const end = useRef<HTMLDivElement>(null);
  const { nextOffset, loading, loadMore } = list;
  // After a failed page the footer offers Try again; scrolling doesn't ask again by itself.
  const failed = list.error !== null && list.error !== undefined;
  useEffect(() => {
    const element = end.current;
    if (!element || nextOffset === null || loading || failed) return;
    if (typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) loadMore();
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [nextOffset, loading, failed, loadMore]);
  return end;
}

/** Shorter lists render every row. */
export const WINDOW_FROM = 150;
/** Rows rendered beyond each edge of the screen. */
const OVERSCAN = 20;

/**
 * The rows `[start, end)` of `count` rows of `rowHeight` px that are on screen, given the top
 * of the list relative to the window and the window's height, with a margin either side.
 */
export function visibleRange(
  top: number,
  viewport: number,
  rowHeight: number,
  count: number,
): [number, number] {
  if (rowHeight <= 0) return [0, count];
  const clamp = (n: number) => Math.min(count, Math.max(0, n));
  const start = clamp(Math.floor(-top / rowHeight) - OVERSCAN);
  const end = clamp(Math.ceil((viewport - top) / rowHeight) + OVERSCAN);
  return [start, Math.max(start, end)];
}

export type RowWindow = {
  start: number;
  end: number;
  rowHeight: number;
};

/**
 * Long lists render only the rows near the screen, with spacer rows for the rest. Attach the
 * returned ref to the list's `<tbody>`. The list scrolls in a different container per layout,
 * so any scroll on the page re-measures. Until a row has a measurable height (and in short
 * lists) every row renders.
 */
export function useWindow(
  count: number,
): [RefObject<HTMLTableSectionElement | null>, RowWindow] {
  const body = useRef<HTMLTableSectionElement>(null);
  const [range, setRange] = useState({ start: 0, end: count, rowHeight: 0 });
  const enabled = count >= WINDOW_FROM;

  useEffect(() => {
    if (!enabled) return;
    let frame = 0;
    const measure = () => {
      frame = 0;
      const element = body.current;
      const row = element?.querySelector<HTMLElement>('tr:not(.spacer-row)');
      const rowHeight = row?.getBoundingClientRect().height ?? 0;
      if (!element || rowHeight <= 0) return;
      const top = element.getBoundingClientRect().top;
      const [start, end] = visibleRange(
        top,
        window.innerHeight,
        rowHeight,
        count,
      );
      setRange((previous) =>
        previous.start === start &&
        previous.end === end &&
        previous.rowHeight === rowHeight
          ? previous
          : { start, end, rowHeight },
      );
    };
    const schedule = () => {
      if (frame === 0) frame = window.requestAnimationFrame(measure);
    };
    schedule();
    document.addEventListener('scroll', schedule, true);
    window.addEventListener('resize', schedule);
    return () => {
      if (frame !== 0) window.cancelAnimationFrame(frame);
      document.removeEventListener('scroll', schedule, true);
      window.removeEventListener('resize', schedule);
    };
  }, [enabled, count]);

  if (!enabled || range.rowHeight <= 0)
    return [body, { start: 0, end: count, rowHeight: 0 }];
  return [
    body,
    {
      start: Math.min(range.start, count),
      end: Math.min(range.end, count),
      rowHeight: range.rowHeight,
    },
  ];
}
