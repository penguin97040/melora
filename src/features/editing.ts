import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent } from 'react';
import { getPlaylistOrder, setPlaylistOrder } from '../native/client';
import type { PlaylistSummary, TrackRow } from '../native/client';
import { describeFailure, isOraError } from './account';
import type { Page } from './library';

/**
 * The loaded rows after Spotify moves the item at position `from` to position `to`: the rows
 * between shift by one. Positions count rows Spotify left out, so they're adjusted, not renumbered.
 */
export function moveRows(
  rows: TrackRow[],
  from: number,
  to: number,
): TrackRow[] {
  if (from === to) return rows;
  return rows
    .map((row) => {
      if (row.position === from) return { ...row, position: to };
      if (to > from && row.position > from && row.position <= to)
        return { ...row, position: row.position - 1 };
      if (to < from && row.position >= to && row.position < from)
        return { ...row, position: row.position + 1 };
      return row;
    })
    .sort((a, b) => a.position - b.position);
}

/** The loaded rows after Spotify removes every copy of `uri`; the rows after each close up. */
export function removeRows(page: Page<TrackRow>, uri: string): Page<TrackRow> {
  const gone = page.items
    .filter((row) => row.uri === uri)
    .map((row) => row.position);
  if (gone.length === 0) return page;
  const items = page.items
    .filter((row) => row.uri !== uri)
    .map((row) => {
      const before = gone.filter((position) => position < row.position).length;
      return before === 0 ? row : { ...row, position: row.position - before };
    });
  return {
    items,
    total: Math.max(0, page.total - gone.length),
    nextOffset:
      page.nextOffset === null
        ? null
        : Math.max(0, page.nextOffset - gone.length),
  };
}

/** `items` with the one at `from` moved to index `to`. */
export function moveIndex<T>(items: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || from >= items.length) return items;
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(Math.max(0, Math.min(to, next.length)), 0, moved!);
  return next;
}

/** Playlists in Melora's order: the saved ones first, then the rest in Spotify's order. */
export function orderPlaylists(
  playlists: PlaylistSummary[],
  order: string[],
): PlaylistSummary[] {
  if (order.length === 0) return playlists;
  const rank = new Map(order.map((id, i) => [id, i]));
  const placed = playlists
    .filter((playlist) => rank.has(playlist.id))
    .sort((a, b) => rank.get(a.id)! - rank.get(b.id)!);
  return [...placed, ...playlists.filter((playlist) => !rank.has(playlist.id))];
}

/**
 * The order to save after the shown playlists were rearranged: their IDs as shown, then saved
 * IDs that aren't loaded yet, in their saved order.
 */
export function savedOrder(
  shown: PlaylistSummary[],
  order: string[],
): string[] {
  const ids = shown.map((playlist) => playlist.id);
  const listed = new Set(ids);
  return [...ids, ...order.filter((id) => !listed.has(id))];
}

/** Playlists the account owns or collaborates on: the ones Spotify lets it change. */
export function editable(playlist: PlaylistSummary): boolean {
  return playlist.readable;
}

/** Copy for a failed playlist change. */
export function editFailure(error: unknown): string | null {
  if (
    isOraError(error) &&
    error.kind === 'forbidden' &&
    error.hint === 'NotOwnerPlaylist'
  )
    return 'Spotify only lets you change playlists you own or collaborate on.';
  return describeFailure(error);
}

export const ORDER_FAILURE =
  'Melora couldn’t save its playlist order. Check that Melora’s data folder is writable.';

/** Alt+↑ and Alt+↓ move the focused row up or down one place. */
export function moveKey(event: KeyboardEvent): -1 | 1 | null {
  if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey)
    return null;
  if (event.key === 'ArrowUp') return -1;
  if (event.key === 'ArrowDown') return 1;
  return null;
}

export type PlaylistOrder = {
  /** Playlist IDs first to last; empty while it's Spotify's order. */
  order: string[];
  save: (order: string[]) => void;
};

/**
 * Melora's own playlist order for `account` (`null` reads nothing), saved beside Melora's other
 * data. Changes show at once; a failed save is reported through `onError`.
 */
export function usePlaylistOrder(
  account: string | null,
  onError: (message: string) => void,
): PlaylistOrder {
  const [state, setState] = useState<{
    account: string | null;
    order: string[];
  }>({ account: null, order: [] });
  // Saves take turns, so the last change made is the one kept.
  const saving = useRef<Promise<unknown>>(Promise.resolve());

  useEffect(() => {
    if (account === null) return;
    let active = true;
    void getPlaylistOrder().then(
      (order) => {
        if (active)
          setState((previous) =>
            previous.account === account ? previous : { account, order },
          );
      },
      () => undefined,
    );
    return () => {
      active = false;
    };
  }, [account]);

  return {
    order: state.account === account && account !== null ? state.order : [],
    save: (order) => {
      if (account === null) return;
      setState({ account, order });
      saving.current = saving.current
        .then(() => setPlaylistOrder(order))
        .catch(() => onError(ORDER_FAILURE));
    },
  };
}
