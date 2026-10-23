import { useEffect, useState } from 'react';
import { likedTracks, listPlaylists, playlistTracks } from '../native/client';
import type {
  PlayRequest,
  PlaylistSummary,
  SpotifyStatus,
  TrackRow,
} from '../native/client';
import type { Feature } from '../native/bindings/Feature';

/** What the sidebar can show in the centre besides the preview playlists. */
export type ListSource =
  { kind: 'liked' } | { kind: 'playlist'; playlist: PlaylistSummary };

export function granted(status: SpotifyStatus | null, feature: Feature) {
  return (
    status?.auth.kind === 'signedIn' && status.auth.granted.includes(feature)
  );
}

/** The key a list's pages are loaded under; Rust caches them too. */
export function sourceKey(source: ListSource): string {
  return source.kind === 'liked' ? 'liked' : `playlist:${source.playlist.id}`;
}

/** Spotify plays at most this many tracks from a list without a context. */
const MAX_ITEMS = 100;

/** A playlist plays from the clicked row; Liked Songs plays the loaded rows from there. */
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

type Page<T> = { items: T[]; total: number; nextOffset: number | null };

export async function fetchPlaylists(_key: string, offset: number) {
  return listPlaylists(offset);
}

export async function fetchTracks(key: string, offset: number) {
  return key === 'liked'
    ? likedTracks(offset)
    : playlistTracks(key.slice('playlist:'.length), offset);
}

type Loaded<T> = Page<T> & {
  key: string | null;
  /** The last offset loaded, or -1. */
  loaded: number;
  error: unknown;
  failed: { offset: number; attempt: number } | null;
};

const nothing = {
  key: null,
  items: [],
  total: 0,
  nextOffset: null,
  loaded: -1,
  error: null,
  failed: null,
};

export type Paged<T> = Page<T> & {
  loading: boolean;
  error: unknown;
  loadMore: () => void;
  retry: () => void;
};

/**
 * Pages of a Spotify list, loaded one at a time as they're asked for. A new `key` starts again;
 * `null` loads nothing. `fetchPage` must be a stable function.
 */
export function usePaged<T>(
  key: string | null,
  fetchPage: (key: string, offset: number) => Promise<Page<T>>,
): Paged<T> {
  const [want, setWant] = useState({ key, offset: 0, attempt: 0 });
  const [data, setData] = useState<Loaded<T>>(nothing);
  if (want.key !== key) setWant({ key, offset: 0, attempt: 0 });

  const { offset, attempt } = want;
  const wantKey = want.key;
  useEffect(() => {
    if (wantKey === null) return;
    let active = true;
    void fetchPage(wantKey, offset).then(
      (page) => {
        if (!active) return;
        setData((previous) => {
          const same = previous.key === wantKey && offset > 0;
          if (same && previous.loaded >= offset) return previous;
          return {
            key: wantKey,
            items: same ? [...previous.items, ...page.items] : page.items,
            total: page.total,
            nextOffset: page.nextOffset,
            loaded: offset,
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
          failed: { offset, attempt },
        }));
      },
    );
    return () => {
      active = false;
    };
  }, [wantKey, offset, attempt, fetchPage]);

  const current: Loaded<T> = data.key === key ? data : nothing;
  const failedNow =
    current.failed?.offset === want.offset &&
    current.failed.attempt === want.attempt;
  // `loaded` is -1 before the first page, so a new list counts as loading straight away.
  const loading = key !== null && current.loaded < want.offset && !failedNow;
  return {
    items: current.items,
    total: current.total,
    nextOffset: current.nextOffset,
    loading,
    error: failedNow ? current.error : null,
    loadMore: () => {
      if (current.nextOffset !== null && !loading)
        setWant({ key, offset: current.nextOffset, attempt: 0 });
    },
    retry: () =>
      setWant((previous) => ({ ...previous, attempt: previous.attempt + 1 })),
  };
}
