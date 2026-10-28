import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import {
  activatePlayback,
  getAppInfo,
  getWindowChrome,
  playFromList,
  savePreferences,
  searchPage,
  searchSpotify,
  showMainWindow,
  subscribeSpotify,
} from './native/client';
import type {
  CatalogItem,
  SearchKind,
  CatalogPage,
  SearchResults,
  SpotifyStatus,
} from './native/client';

vi.mock('./native/client', () => ({
  defaultPreferences: { scale: 100, comfortable: false, volume: 70 },
  getAppInfo: vi.fn(),
  getWindowChrome: vi.fn(),
  savePreferences: vi.fn(),
  showMainWindow: vi.fn(),
  hasNativeStorage: () => true,
  subscribeSpotify: vi.fn(),
  subscribePlayback: vi.fn(async (onStatus: (status: unknown) => void) => {
    const { playbackStatus } = await import('./native/playbackFixture');
    onStatus(playbackStatus());
  }),
  openInSpotify: vi.fn(),
  activatePlayback: vi.fn(),
  enablePlayback: vi.fn(),
  refreshPlayback: vi.fn(),
  refreshLiked: vi.fn(),
  loadSpotifyProfile: vi.fn(),
  allowFeature: vi.fn(),
  allowLibrary: vi.fn(),
  listPlaylists: vi.fn(),
  getPlaylistOrder: vi.fn(async () => []),
  setPlaylistOrder: vi.fn(async () => undefined),
  playlistTracks: vi.fn(),
  likedTracks: vi.fn(),
  playFromList: vi.fn(),
  searchSpotify: vi.fn(),
  searchPage: vi.fn(),
}));

function signedIn(): SpotifyStatus {
  return {
    auth: {
      kind: 'signedIn',
      profile: {
        id: 'user1',
        accountId: 'acct1',
        displayName: 'Kiri',
        imageUrl: null,
        spotifyUrl: null,
      },
      sessionOnly: false,
      consenting: false,
      granted: ['SignIn', 'Search'],
    },
    clientIdFromEnvironment: false,
    clientIdHint: null,
    lastError: null,
    redirectUri: 'http://127.0.0.1:43821/callback',
  };
}

function item(
  kind: SearchKind,
  position: number,
  name: string,
  more: Partial<CatalogItem> = {},
): CatalogItem {
  const id = `${kind.slice(0, 4)}${position}`.padEnd(22, 'x');
  return {
    position,
    uri: `spotify:${kind}:${id}`,
    name,
    by: [],
    album: '',
    releaseDate: '',
    total: null,
    durationMs: null,
    playable: true,
    spotifyUrl: `https://open.spotify.com/${kind}/${id}`,
    ...more,
  };
}

function page(items: CatalogItem[], total = items.length): CatalogPage {
  return { items, total, nextOffset: total > items.length ? 10 : null };
}

const SONGS = Array.from({ length: 10 }, (_, i) =>
  item('track', i, `Song ${i + 1}`, {
    by: ['Kiri', 'Te'],
    album: 'Record',
    durationMs: 200_000,
    playable: i !== 1,
  }),
);

const RESULTS: SearchResults = {
  tracks: page(SONGS, 250),
  artists: page([item('artist', 0, 'Kiri Band')]),
  albums: page([
    item('album', 0, 'Record', {
      by: ['Kiri'],
      releaseDate: '2021-03-04',
      total: 12,
    }),
  ]),
  playlists: page([]),
  shows: page([]),
  episodes: page([
    item('episode', 0, 'Episode one', {
      releaseDate: '2026-10-01',
      durationMs: 3_600_000,
    }),
  ]),
};

const NOTHING: SearchResults = {
  tracks: page([]),
  artists: page([]),
  albums: page([]),
  playlists: page([]),
  shows: page([]),
  episodes: page([]),
};

function section(name: string) {
  return within(screen.getByRole('region', { name }));
}

async function openSearch(user: ReturnType<typeof userEvent.setup>) {
  render(<App />);
  await user.click(await screen.findByRole('button', { name: /Search/ }));
  return screen.findByLabelText('Search Spotify');
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getWindowChrome).mockResolvedValue('preview');
  vi.mocked(getAppInfo).mockResolvedValue({
    version: '0.5.3',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
  vi.mocked(showMainWindow).mockResolvedValue(undefined);
  vi.mocked(playFromList).mockResolvedValue(undefined);
  vi.mocked(searchSpotify).mockResolvedValue(RESULTS);
  vi.mocked(subscribeSpotify).mockImplementation(async (onStatus) =>
    onStatus(signedIn()),
  );
});

describe('Spotify search', () => {
  it('asks a signed-out user to sign in before searching', async () => {
    vi.mocked(subscribeSpotify).mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole('button', { name: /Search/ }));
    expect(
      screen.getByText(/Sign in with Spotify to search songs/),
    ).toBeVisible();
    expect(screen.queryByLabelText('Search Spotify')).not.toBeInTheDocument();
    expect(searchSpotify).not.toHaveBeenCalled();
  });

  it('asks once typing settles and lists each kind of result', async () => {
    const user = userEvent.setup();
    const input = await openSearch(user);
    expect(screen.getByText(/Type to search Spotify/)).toBeVisible();
    await user.type(input, ' kiri ');
    expect(await screen.findByRole('region', { name: 'Songs' })).toBeVisible();
    expect(searchSpotify).toHaveBeenCalledTimes(1);
    expect(searchSpotify).toHaveBeenCalledWith('kiri');

    // Five songs show; the rest are behind "See all". Kinds without results are left out.
    expect(section('Songs').getAllByRole('row')).toHaveLength(6);
    expect(
      section('Songs').getByRole('button', { name: 'See all 250 songs' }),
    ).toBeVisible();
    expect(section('Albums').getByText('2021')).toBeVisible();
    expect(section('Albums').getByText('12')).toBeVisible();
    expect(section('Episodes').getByText('01/10/2026')).toBeVisible();
    expect(section('Episodes').getByText('1:00:00')).toBeVisible();
    expect(section('Artists').getByText('Top tracks')).toBeVisible();
    expect(
      screen.queryByRole('region', { name: 'Playlists' }),
    ).not.toBeInTheDocument();
    expect(screen.getAllByText('From Spotify').length).toBeGreaterThan(0);

    // Songs Spotify can't play in the account's country can't be chosen.
    expect(
      section('Songs').getByRole('button', { name: /Song 2/ }),
    ).toBeDisabled();
  });

  it('plays songs from the one chosen, and other kinds as themselves', async () => {
    const user = userEvent.setup();
    await user.type(await openSearch(user), 'kiri');
    await user.click(
      await screen.findByRole('button', { name: /^Song 1(?!\d)/ }),
    );
    expect(activatePlayback).toHaveBeenCalled();
    // The unplayable second song is left out; the songs beyond the five shown still play.
    expect(playFromList).toHaveBeenLastCalledWith({
      kind: 'items',
      uris: SONGS.filter((song) => song.playable).map((song) => song.uri),
    });
    await user.click(screen.getByRole('button', { name: /^Record/ }));
    expect(playFromList).toHaveBeenLastCalledWith({
      kind: 'context',
      uri: RESULTS.albums.items[0]!.uri,
      position: null,
    });
    await user.click(screen.getByRole('button', { name: /^Kiri Band/ }));
    expect(playFromList).toHaveBeenLastCalledWith({
      kind: 'context',
      uri: RESULTS.artists.items[0]!.uri,
      position: null,
    });
    await user.click(screen.getByRole('button', { name: /^Episode one/ }));
    expect(playFromList).toHaveBeenLastCalledWith({
      kind: 'items',
      uris: [RESULTS.episodes.items[0]!.uri],
    });
  });

  it('pages through every result of one kind', async () => {
    const more = Array.from({ length: 10 }, (_, i) =>
      item('track', 10 + i, `More ${i + 1}`),
    );
    vi.mocked(searchPage).mockImplementation(async (_query, _kind, offset) =>
      offset === 0
        ? RESULTS.tracks
        : { items: more, total: 250, nextOffset: 20 },
    );
    const user = userEvent.setup();
    await user.type(await openSearch(user), 'kiri');
    await user.click(
      await screen.findByRole('button', { name: 'See all 250 songs' }),
    );
    expect(await screen.findByRole('heading', { name: 'Songs' })).toBeVisible();
    expect(searchPage).toHaveBeenCalledWith('kiri', 'track', 0);
    expect(screen.getByText('250 songs')).toBeVisible();
    await user.click(
      await screen.findByRole('button', { name: 'Show more songs' }),
    );
    expect(
      await screen.findByRole('button', { name: /^More 1(?!\d)/ }),
    ).toBeVisible();
    expect(searchPage).toHaveBeenCalledWith('kiri', 'track', 10);
    expect(screen.getByText('11')).toBeVisible();

    await user.click(
      screen.getByRole('button', { name: 'Back to all results' }),
    );
    expect(await screen.findByRole('region', { name: 'Songs' })).toBeVisible();
  });

  it('ignores a search a newer one replaced, and offers to retry a failure', async () => {
    vi.mocked(searchSpotify)
      .mockRejectedValueOnce({ kind: 'cancelled' })
      .mockRejectedValueOnce({ kind: 'offline' })
      .mockResolvedValue(NOTHING);
    const user = userEvent.setup();
    const input = await openSearch(user);
    await user.type(input, 'ab');
    await waitFor(() => expect(searchSpotify).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();

    await user.type(input, 'c');
    expect(await screen.findByText(/Spotify can’t be reached/)).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(
      await screen.findByText(
        'Spotify found nothing for “abc”. Try other words.',
      ),
    ).toBeVisible();
    expect(searchSpotify).toHaveBeenLastCalledWith('abc');
  });
});
