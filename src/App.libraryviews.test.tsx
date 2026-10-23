import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import {
  albumTracks,
  allowFeature,
  getAppInfo,
  getWindowChrome,
  likedTracks,
  listPlaylists,
  playFromList,
  savePreferences,
  savedAlbums,
  savedEpisodes,
  savedShows,
  savedStates,
  searchSpotify,
  setSaved,
  showEpisodes,
  showMainWindow,
  subscribeSpotify,
} from './native/client';
import type {
  CatalogItem,
  CatalogPage,
  SpotifyStatus,
  TrackPage,
  TrackRow,
} from './native/client';
import type { Feature } from './native/bindings/Feature';

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
  savedAlbums: vi.fn(),
  savedShows: vi.fn(),
  savedEpisodes: vi.fn(),
  albumTracks: vi.fn(),
  showEpisodes: vi.fn(),
  savedStates: vi.fn(),
  setSaved: vi.fn(),
  playFromList: vi.fn(),
  searchSpotify: vi.fn(),
  searchPage: vi.fn(),
}));

const EVERYTHING: Feature[] = [
  'SignIn',
  'Search',
  'Playlists',
  'PlaylistDetail',
  'SavedTracks',
  'SavedAlbums',
  'SavedShowsEpisodes',
  'LibrarySaveRemove',
];

function signedIn(granted: Feature[]): SpotifyStatus {
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
      granted,
    },
    clientIdFromEnvironment: false,
    clientIdHint: null,
    lastError: null,
    redirectUri: 'http://127.0.0.1:43821/callback',
  };
}

function signIn(granted: Feature[]) {
  vi.mocked(subscribeSpotify).mockImplementation(async (onStatus) =>
    onStatus(signedIn(granted)),
  );
}

const ID = (prefix: string, n: number) => `${prefix}${n}`.padEnd(22, 'x');

function catalog(
  kind: 'album' | 'show',
  position: number,
  name: string,
  more: Partial<CatalogItem> = {},
): CatalogItem {
  const id = ID(kind, position);
  return {
    position,
    uri: `spotify:${kind}:${id}`,
    name,
    by: [kind === 'album' ? 'Kiri' : 'Kiri Media'],
    album: '',
    releaseDate: '',
    total: 12,
    durationMs: null,
    playable: true,
    spotifyUrl: `https://open.spotify.com/${kind}/${id}`,
    ...more,
  };
}

function row(
  kind: 'track' | 'episode',
  position: number,
  name: string,
  more: Partial<TrackRow> = {},
): TrackRow {
  const id = ID(kind, position);
  return {
    position,
    uri: `spotify:${kind}:${id}`,
    name,
    artists: kind === 'track' ? ['Kiri'] : [],
    album: '',
    releaseDate: '',
    durationMs: 200_000,
    spotifyUrl: `https://open.spotify.com/${kind}/${id}`,
    ...more,
  };
}

function tracks(items: TrackRow[]): TrackPage {
  return { items, total: items.length, nextOffset: null };
}

function items(list: CatalogItem[]): CatalogPage {
  return { items: list, total: list.length, nextOffset: null };
}

const ALBUM = catalog('album', 0, 'Record', { releaseDate: '2021-03-04' });
const SHOW = catalog('show', 0, 'Talk', { total: 120 });

async function openSidebar(name: string) {
  const user = userEvent.setup();
  render(<App />);
  const sidebar = await screen.findByRole('region', {
    name: 'Your Spotify playlists',
  });
  await user.click(
    within(sidebar).getByRole('button', { name: new RegExp(name) }),
  );
  return user;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getWindowChrome).mockResolvedValue('preview');
  vi.mocked(getAppInfo).mockResolvedValue({
    version: '0.5.4',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
  vi.mocked(showMainWindow).mockResolvedValue(undefined);
  vi.mocked(playFromList).mockResolvedValue(undefined);
  vi.mocked(setSaved).mockResolvedValue(undefined);
  vi.mocked(listPlaylists).mockResolvedValue({
    items: [],
    total: 0,
    nextOffset: null,
  });
  vi.mocked(savedStates).mockImplementation(async (uris) =>
    uris.map((uri) => uri === ALBUM.uri),
  );
  signIn(EVERYTHING);
});

describe('Saved albums, podcasts and episodes', () => {
  it('lists saved albums and opens one to play and save its tracks', async () => {
    vi.mocked(savedAlbums).mockResolvedValue(items([ALBUM]));
    vi.mocked(albumTracks).mockResolvedValue(
      tracks([row('track', 0, 'Opening'), row('track', 1, 'Second')]),
    );
    const user = await openSidebar('Albums');
    expect(
      await screen.findByRole('heading', { name: 'Albums' }),
    ).toBeVisible();
    expect(screen.getByText('2021')).toBeVisible();
    await user.click(screen.getByRole('button', { name: /^Record/ }));

    expect(
      await screen.findByRole('heading', { name: 'Record' }),
    ).toBeVisible();
    expect(albumTracks).toHaveBeenCalledWith(ALBUM.uri.split(':')[2], 0);
    expect(screen.getByText('By Kiri, 2021.', { exact: false })).toBeVisible();
    // The album and its tracks are each asked about once; the album is saved, the tracks
    // aren't.
    await waitFor(() =>
      expect(vi.mocked(savedStates).mock.calls.flat(2).sort()).toEqual(
        [ALBUM.uri, row('track', 0, '').uri, row('track', 1, '').uri].sort(),
      ),
    );
    expect(
      await screen.findByRole('button', {
        name: 'Remove “Record” from Your Library',
      }),
    ).toBeVisible();
    expect(screen.getByText('In Your Library')).toBeVisible();

    // A track plays within the album, from its place.
    await user.click(screen.getByRole('button', { name: /^Second/ }));
    expect(playFromList).toHaveBeenLastCalledWith({
      kind: 'context',
      uri: ALBUM.uri,
      position: 1,
    });

    // Its + adds it to Liked Songs, and then shows it's there.
    await user.click(
      screen.getByRole('button', { name: 'Add “Opening” to Liked Songs' }),
    );
    expect(setSaved).toHaveBeenCalledWith([row('track', 0, '').uri], true);
    expect(
      await screen.findByRole('button', {
        name: 'Remove “Opening” from Liked Songs',
      }),
    ).toBeVisible();

    // The album itself can be removed from the library.
    await user.click(
      screen.getByRole('button', { name: 'Remove “Record” from Your Library' }),
    );
    expect(setSaved).toHaveBeenLastCalledWith([ALBUM.uri], false);
    expect(await screen.findByText('Save')).toBeVisible();
  });

  it('opens a podcast and plays one episode at a time', async () => {
    vi.mocked(savedShows).mockResolvedValue(items([SHOW]));
    vi.mocked(showEpisodes).mockResolvedValue(
      tracks([
        row('episode', 0, 'Newest', { releaseDate: '2026-10-02' }),
        row('episode', 1, 'Older', { releaseDate: '2026-09-25' }),
      ]),
    );
    const user = await openSidebar('Podcasts');
    await user.click(await screen.findByRole('button', { name: /^Talk/ }));
    expect(await screen.findByRole('heading', { name: 'Talk' })).toBeVisible();
    expect(
      screen.getByText('From Kiri Media.', { exact: false }),
    ).toBeVisible();
    expect(screen.getByText('02/10/2026')).toBeVisible();
    // Episodes have no + of their own.
    expect(
      screen.queryByRole('button', { name: /to Liked Songs/ }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /^Older/ }));
    expect(playFromList).toHaveBeenLastCalledWith({
      kind: 'items',
      uris: [row('episode', 1, '').uri],
    });
    expect(
      screen.getByRole('button', { name: 'Save “Talk” to Your Library' }),
    ).toBeVisible();
  });

  it('asks for the podcast permission before showing saved episodes', async () => {
    signIn(EVERYTHING.filter((feature) => feature !== 'SavedShowsEpisodes'));
    vi.mocked(savedEpisodes)
      .mockRejectedValueOnce({
        kind: 'needsScope',
        feature: 'SavedShowsEpisodes',
      })
      .mockResolvedValue(
        tracks([row('episode', 0, 'Saved one', { album: 'Talk' })]),
      );
    vi.mocked(allowFeature).mockResolvedValue(signedIn(EVERYTHING));
    const user = await openSidebar('Your Episodes');
    await user.click(await screen.findByRole('button', { name: 'Allow' }));
    expect(allowFeature).toHaveBeenCalledWith('SavedShowsEpisodes');
    expect(
      await screen.findByRole('button', { name: /^Saved one/ }),
    ).toBeVisible();
    expect(screen.getByText('Talk')).toBeVisible();
  });

  it('shows Liked Songs as liked without asking', async () => {
    vi.mocked(likedTracks).mockResolvedValue(tracks([row('track', 0, 'Mine')]));
    const user = await openSidebar('Liked Songs');
    await user.click(
      await screen.findByRole('button', {
        name: 'Remove “Mine” from Liked Songs',
      }),
    );
    expect(setSaved).toHaveBeenCalledWith([row('track', 0, '').uri], false);
    expect(savedStates).not.toHaveBeenCalled();
    expect(
      await screen.findByRole('button', { name: 'Add “Mine” to Liked Songs' }),
    ).toBeVisible();
  });

  it('opens a podcast found by search', async () => {
    const empty = items([]);
    vi.mocked(searchSpotify).mockResolvedValue({
      tracks: empty,
      artists: empty,
      albums: empty,
      playlists: empty,
      shows: items([SHOW]),
      episodes: empty,
    });
    vi.mocked(showEpisodes).mockResolvedValue(tracks([]));
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole('button', { name: /Search/ }));
    await user.type(await screen.findByLabelText('Search Spotify'), 'talk');
    const podcasts = await screen.findByRole('region', { name: 'Podcasts' });
    await user.click(within(podcasts).getByRole('button', { name: /^Talk/ }));
    expect(await screen.findByRole('heading', { name: 'Talk' })).toBeVisible();
    expect(showEpisodes).toHaveBeenCalledWith(SHOW.uri.split(':')[2], 0);
    expect(playFromList).not.toHaveBeenCalled();
    expect(
      await screen.findByText('This podcast has no episodes.'),
    ).toBeVisible();
  });
});
