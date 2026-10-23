import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import {
  addToQueue,
  albumTracks,
  allowFeature,
  getAppInfo,
  getQueue,
  getWindowChrome,
  likedTracks,
  listDevices,
  listPlaylists,
  playFromList,
  recentlyPlayed,
  savePreferences,
  savedAlbums,
  savedStates,
  showMainWindow,
  subscribePlayback,
  subscribeSpotify,
} from './native/client';
import type {
  CatalogItem,
  PlaybackStatus,
  SpotifyStatus,
  TrackPage,
  TrackRow,
} from './native/client';
import type { Feature } from './native/bindings/Feature';
import { playbackStatus } from './native/playbackFixture';

vi.mock('./native/client', () => ({
  defaultPreferences: { scale: 100, comfortable: false, volume: 70 },
  getAppInfo: vi.fn(),
  getWindowChrome: vi.fn(),
  savePreferences: vi.fn(),
  showMainWindow: vi.fn(),
  hasNativeStorage: () => true,
  subscribeSpotify: vi.fn(),
  subscribePlayback: vi.fn(),
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
  listDevices: vi.fn(),
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
  getQueue: vi.fn(),
  addToQueue: vi.fn(),
  recentlyPlayed: vi.fn(),
}));

const EVERYTHING: Feature[] = [
  'SignIn',
  'Playlists',
  'PlaylistDetail',
  'SavedTracks',
  'SavedAlbums',
  'LibrarySaveRemove',
  'RecentlyPlayed',
  'Queue',
  'AddToQueue',
  'DevicePicker',
];

function status(
  auth: SpotifyStatus['auth'] = { kind: 'signedOut' },
): SpotifyStatus {
  return {
    auth,
    clientIdFromEnvironment: false,
    clientIdHint: 'abcd',
    lastError: null,
    redirectUri: 'http://127.0.0.1:43821/callback',
  };
}

function signIn(granted: Feature[]) {
  vi.mocked(subscribeSpotify).mockImplementation(async (onStatus) =>
    onStatus(
      status({
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
      }),
    ),
  );
}

function play(extra: Partial<PlaybackStatus>) {
  vi.mocked(subscribePlayback).mockImplementation(async (onStatus) =>
    onStatus(playbackStatus(extra)),
  );
}

const ID = (prefix: string, n: number) => `${prefix}${n}`.padEnd(22, 'x');

function row(position: number, name: string, kind = 'track'): TrackRow {
  const id = ID(kind, position);
  return {
    position,
    uri: `spotify:${kind}:${id}`,
    name,
    artists: kind === 'track' ? ['Kiri'] : [],
    album: kind === 'track' ? 'Record' : 'Talk',
    releaseDate: kind === 'track' ? '' : '2026-10-02',
    durationMs: 200_000,
    spotifyUrl: `https://open.spotify.com/${kind}/${id}`,
  };
}

function tracks(items: TrackRow[]): TrackPage {
  return { items, total: items.length, nextOffset: null };
}

const ALBUM: CatalogItem = {
  position: 0,
  uri: `spotify:album:${ID('album', 0)}`,
  name: 'Saved record',
  by: ['Kiri'],
  album: '',
  releaseDate: '2021-03-04',
  total: 9,
  durationMs: null,
  playable: true,
  spotifyUrl: `https://open.spotify.com/album/${ID('album', 0)}`,
};

const PHONE = {
  tier: 'ready' as const,
  device: {
    key: 'k1',
    name: 'Kitchen',
    kind: 'smartphone' as const,
    active: true,
    restricted: false,
    thisApp: false,
    supportsVolume: false,
    volumePercent: null,
  },
  nowPlaying: {
    name: 'Playing song',
    artists: ['Kiri'],
    album: 'Record',
    spotifyUrl: `https://open.spotify.com/track/${ID('track', 9)}`,
    imageUrl: null,
    liked: null,
  },
  paused: false,
  durationMs: 200_000,
};

async function openNav(name: string) {
  const user = userEvent.setup();
  render(<App />);
  await user.click(
    await screen.findByRole('button', { name: new RegExp(name) }),
  );
  return user;
}

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
    version: '0.5.5',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
  vi.mocked(showMainWindow).mockResolvedValue(undefined);
  vi.mocked(playFromList).mockResolvedValue(undefined);
  vi.mocked(addToQueue).mockResolvedValue(undefined);
  vi.mocked(albumTracks).mockResolvedValue(tracks([row(0, 'Album track')]));
  vi.mocked(savedStates).mockImplementation(async (uris) =>
    uris.map(() => false),
  );
  vi.mocked(listPlaylists).mockResolvedValue({
    items: [],
    total: 0,
    nextOffset: null,
  });
  vi.mocked(savedAlbums).mockResolvedValue({
    items: [ALBUM],
    total: 1,
    nextOffset: null,
  });
  vi.mocked(listDevices).mockResolvedValue([PHONE.device]);
  vi.mocked(recentlyPlayed).mockResolvedValue(
    tracks([row(0, 'Last one'), row(1, 'Before that'), row(2, 'Earlier')]),
  );
  play(PHONE);
  signIn(EVERYTHING);
});

describe('Home', () => {
  it('comes first in the navigation, while Melora still opens on Library', async () => {
    render(<App />);
    const nav = await screen.findByRole('navigation', {
      name: 'Main navigation',
    });
    const names = within(nav)
      .getAllByRole('button')
      .map((button) => button.textContent);
    expect(names.slice(0, 4)).toEqual([
      '01Home›',
      '02Library›',
      '03Search›',
      '04Settings›',
    ]);
    expect(
      within(nav).getByRole('button', { name: /Library/ }),
    ).toHaveAttribute('aria-current', 'page');
  });

  it('shows what’s playing, what played recently, the library and devices', async () => {
    const user = await openNav('Home');
    expect(
      await screen.findByRole('heading', { name: 'Home' }),
    ).toBeInTheDocument();
    const now = screen.getByRole('region', { name: 'Now playing' });
    expect(within(now).getByText('Playing song')).toBeInTheDocument();

    const recent = screen.getByRole('region', { name: 'Recently played' });
    await user.click(
      await within(recent).findByRole('button', { name: /Before that/ }),
    );
    // Like Liked Songs: the chosen track and those after it.
    expect(playFromList).toHaveBeenCalledWith({
      kind: 'items',
      uris: [row(1, '').uri, row(2, '').uri],
    });

    const devices = screen.getByRole('region', { name: 'Devices' });
    expect(await within(devices).findByText('Kitchen')).toBeInTheDocument();
    expect(within(devices).getByText('Active')).toBeInTheDocument();

    const albums = screen.getByRole('region', { name: 'Saved albums' });
    await user.click(
      await within(albums).findByRole('button', { name: /Saved record/ }),
    );
    expect(
      await screen.findByRole('heading', { name: 'Saved record' }),
    ).toBeInTheDocument();
  });

  it('offers to ask for what it may not show yet', async () => {
    signIn(['SignIn', 'Playlists']);
    vi.mocked(allowFeature).mockResolvedValue(status());
    const user = await openNav('Home');
    const recent = await screen.findByRole('region', {
      name: 'Recently played',
    });
    await user.click(within(recent).getByRole('button', { name: 'Allow' }));
    expect(allowFeature).toHaveBeenCalledWith('RecentlyPlayed');
    expect(recentlyPlayed).not.toHaveBeenCalled();
    expect(
      screen.queryByRole('region', { name: 'Saved albums' }),
    ).not.toBeInTheDocument();
  });

  it('asks a signed-out user to sign in', async () => {
    vi.mocked(subscribeSpotify).mockImplementation(async (onStatus) =>
      onStatus(status()),
    );
    play({});
    await openNav('Home');
    expect(
      await screen.findByText(/Sign in with Spotify to see your Home/),
    ).toBeInTheDocument();
    expect(recentlyPlayed).not.toHaveBeenCalled();
    expect(listDevices).not.toHaveBeenCalled();
  });
});

describe('Recently Played and the queue', () => {
  it('lists recently played tracks from the sidebar', async () => {
    await openSidebar('Recently Played');
    expect(
      await screen.findByRole('heading', { name: 'Recently Played' }),
    ).toBeInTheDocument();
    expect(await screen.findByText('Earlier')).toBeInTheDocument();
    expect(recentlyPlayed).toHaveBeenCalledTimes(1);
  });

  it('shows the queue without controls Spotify doesn’t allow', async () => {
    vi.mocked(getQueue).mockResolvedValue({
      current: row(0, 'Playing song'),
      next: [row(0, 'Up next'), row(1, 'An episode', 'episode')],
    });
    const user = await openSidebar('Queue');
    expect(
      await screen.findByRole('heading', { name: 'Queue' }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        'Spotify doesn’t allow reordering the queue from other apps.',
      ),
    ).toBeInTheDocument();
    const next = await screen.findByRole('region', { name: 'Next up' });
    expect(within(next).getByText('Up next')).toBeInTheDocument();
    expect(within(next).getByText('Talk')).toBeInTheDocument();
    // Nothing to click, drag or remove.
    expect(within(next).queryAllByRole('button')).toHaveLength(0);
    expect(getQueue).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(getQueue).toHaveBeenCalledTimes(2));
  });

  it('says when nothing is playing', async () => {
    vi.mocked(getQueue).mockResolvedValue({ current: null, next: [] });
    await openSidebar('Queue');
    expect(
      await screen.findByText(/Nothing is playing\. Start something/),
    ).toBeInTheDocument();
  });
});

describe('Add to queue', () => {
  beforeEach(() => {
    vi.mocked(likedTracks).mockResolvedValue(
      tracks([row(0, 'Liked one'), row(1, 'Liked two')]),
    );
  });

  it('adds a song from its context menu', async () => {
    const user = await openSidebar('Liked Songs');
    const song = await screen.findByRole('button', { name: /^Liked two/ });
    await user.pointer({ keys: '[MouseRight]', target: song });
    const menu = screen.getByRole('menu', { name: 'Actions for Liked two' });
    const item = within(menu).getByRole('menuitem', { name: 'Add to queue' });
    expect(item).toHaveFocus();
    await user.click(item);
    expect(addToQueue).toHaveBeenCalledWith(row(1, '').uri);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(await screen.findByText('Added to queue')).toBeInTheDocument();
  });

  it('opens from the keyboard and Escape gives focus back to the row', async () => {
    const user = await openSidebar('Liked Songs');
    const song = await screen.findByRole('button', { name: /^Liked one/ });
    song.focus();
    // The Menu key and Shift+F10 send a context menu event at 0, 0.
    fireEvent.contextMenu(song, { clientX: 0, clientY: 0 });
    expect(
      screen.getByRole('menuitem', { name: 'Add to queue' }),
    ).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(song).toHaveFocus();
    expect(addToQueue).not.toHaveBeenCalled();
  });

  it('asks for the permission first', async () => {
    signIn(['SignIn', 'Playlists', 'SavedTracks']);
    vi.mocked(allowFeature).mockResolvedValue(status());
    const user = await openSidebar('Liked Songs');
    await user.pointer({
      keys: '[MouseRight]',
      target: await screen.findByRole('button', { name: /^Liked one/ }),
    });
    await user.click(screen.getByRole('menuitem', { name: 'Add to queue' }));
    expect(allowFeature).toHaveBeenCalledWith('AddToQueue');
    // The permission wasn't given, so nothing is sent.
    expect(addToQueue).not.toHaveBeenCalled();
  });

  it('shows Spotify’s refusal when nothing is playing', async () => {
    vi.mocked(addToQueue).mockRejectedValue({ kind: 'noActiveDevice' });
    const user = await openSidebar('Liked Songs');
    await user.pointer({
      keys: '[MouseRight]',
      target: await screen.findByRole('button', { name: /^Liked one/ }),
    });
    await user.click(screen.getByRole('menuitem', { name: 'Add to queue' }));
    expect(
      await screen.findByText(/No Spotify device is active/),
    ).toBeInTheDocument();
  });
});
