import { describe, expect, it } from 'vitest';
import type { PlaylistSummary, TrackRow } from '../native/client';
import {
  moveIndex,
  moveRows,
  orderPlaylists,
  removeRows,
  savedOrder,
} from './editing';
import { overIndex } from './reorder';

function row(position: number, name: string, uri = `spotify:track:${name}`) {
  return { position, name, uri } as TrackRow;
}

const names = (rows: TrackRow[]) =>
  rows.map((item) => `${item.position}:${item.name}`);

function playlist(id: string): PlaylistSummary {
  return { id } as PlaylistSummary;
}

describe('playlist edits', () => {
  it('moves rows as Spotify does, keeping the gaps of rows it left out', () => {
    // Position 2 is a row Spotify sent as null.
    const rows = [row(0, 'a'), row(1, 'b'), row(3, 'd'), row(4, 'e')];
    expect(names(moveRows(rows, 0, 3))).toEqual(['0:b', '2:d', '3:a', '4:e']);
    expect(names(moveRows(rows, 4, 1))).toEqual(['0:a', '1:e', '2:b', '4:d']);
    expect(moveRows(rows, 1, 1)).toBe(rows);
  });

  it('removes every copy of a song and closes the gaps', () => {
    const page = {
      items: [row(0, 'a'), row(1, 'b'), row(2, 'a'), row(3, 'c')],
      total: 60,
      nextOffset: 50,
    };
    const after = removeRows(page, 'spotify:track:a');
    expect(names(after.items)).toEqual(['0:b', '1:c']);
    expect([after.total, after.nextOffset]).toEqual([58, 48]);
    expect(removeRows(page, 'spotify:track:z')).toBe(page);
  });

  it('moves items by index', () => {
    expect(moveIndex(['a', 'b', 'c'], 0, 2)).toEqual(['b', 'c', 'a']);
    expect(moveIndex(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
    expect(moveIndex(['a'], 3, 0)).toEqual(['a']);
  });
});

describe('Melora’s playlist order', () => {
  const lists = ['p1', 'p2', 'p3', 'p4'].map(playlist);

  it('puts saved playlists first, then the rest in Spotify’s order', () => {
    expect(
      orderPlaylists(lists, ['p3', 'gone', 'p1']).map((p) => p.id),
    ).toEqual(['p3', 'p1', 'p2', 'p4']);
    expect(orderPlaylists(lists, [])).toBe(lists);
  });

  it('saves the shown order and keeps saved playlists not loaded yet', () => {
    expect(savedOrder(lists.slice(0, 2).reverse(), ['p9', 'p1'])).toEqual([
      'p2',
      'p1',
      'p9',
    ]);
  });
});

describe('dragging', () => {
  const boxes = [
    { top: 0, bottom: 40 },
    { top: 40, bottom: 80 },
    { top: 80, bottom: 120 },
  ];

  it('finds the item under the pointer, clamped to the ends', () => {
    expect(overIndex(-10, boxes)).toBe(0);
    expect(overIndex(50, boxes)).toBe(1);
    expect(overIndex(500, boxes)).toBe(2);
  });
});
