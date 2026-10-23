import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import {
  addToPlaylist,
  allowFeature,
  createPlaylist,
  getAppInfo,
  getPlaylistOrder,
  getWindowChrome,
  likedTracks,
  listPlaylists,
  moveInPlaylist,
  playFromList,
  playlistTracks,
  removeFromPlaylist,
  savePreferences,
  savedStates,
  setPlaylistOrder,
  showMainWindow,
  subscribePlayback,
  subscribeSpotify,
} from './native/client';
import type {
  PlaylistSummary,
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
  getPlaylistOrder: vi.fn(),
  setPlaylistOrder: vi.fn(),
  playlistTracks: vi.fn(),
  likedTracks: vi.fn(),
  savedStates: vi.fn(),
  setSaved: vi.fn(),
  playFromList: vi.fn(),
  addToQueue: vi.fn(),
  createPlaylist: vi.fn(),
  addToPlaylist: vi.fn(),
  removeFromPlaylist: vi.fn(),
  moveInPlaylist: vi.fn(),
}));

const EDITING: Feature[] = [
  'SignIn',
  'Playlists',
  'PlaylistDetail',
  'SavedTracks',
  'LibrarySaveRemove',
  'PlaylistEdit',
  'CreatePlaylist',
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

const ID = (prefix: string, n: number) => `${prefix}${n}`.padEnd(22, 'x');

function playlist(n: number, name: string, readable = true): PlaylistSummary {
  return {
    id: ID('list', n),
    name,
    owner: readable ? 'Kiri' : 'Someone',
    total: 3,
    imageUrl: null,
    spotifyUrl: `https://open.spotify.com/playlist/${ID('list', n)}`,
    readable,
  };
}

const MINE = playlist(1, 'Mine');
const SECOND = playlist(2, 'Second');
const THEIRS = playlist(3, 'Theirs', false);

function row(position: number, name: string): TrackRow {
  const id = ID('track', position);
  return {
    position,
    uri: `spotify:track:${id}`,
    name,
    artists: ['Kiri'],
    album: 'Record',
    releaseDate: '',
    durationMs: 200_000,
    spotifyUrl: `https://open.spotify.com/track/${id}`,
  };
}

function tracks(items: TrackRow[]): TrackPage {
  return { items, total: items.length, nextOffset: null };
}

const SONGS = [row(0, 'Alpha'), row(1, 'Bravo'), row(2, 'Charlie')];

async function openSidebar(name: string) {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await sidebarButton(name));
  return user;
}

async function sidebarButton(name: string) {
  const sidebar = await screen.findByRole('region', {
    name: 'Your Spotify playlists',
  });
  return within(sidebar).findByRole('button', {
    name: new RegExp(`^.?${name}`),
  });
}

/** The songs' names, top to bottom. */
function songOrder(): string[] {
  return screen
    .getAllByRole('button', { name: /^(Alpha|Bravo|Charlie)/ })
    .map((button) => button.textContent?.replace('Kiri', '') ?? '');
}

function sidebarOrder(): string[] {
  const sidebar = screen.getByRole('region', {
    name: 'Your Spotify playlists',
  });
  return [...sidebar.querySelectorAll('[data-reorder] .playlist-name')].map(
    (name) => name.textContent ?? '',
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getWindowChrome).mockResolvedValue('preview');
  vi.mocked(getAppInfo).mockResolvedValue({
    version: '0.6.0',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
  vi.mocked(showMainWindow).mockResolvedValue(undefined);
  vi.mocked(playFromList).mockResolvedValue(undefined);
  vi.mocked(savedStates).mockImplementation(async (uris) =>
    uris.map(() => false),
  );
  vi.mocked(listPlaylists).mockResolvedValue({
    items: [MINE, SECOND, THEIRS],
    total: 3,
    nextOffset: null,
  });
  vi.mocked(playlistTracks).mockResolvedValue(tracks(SONGS));
  vi.mocked(likedTracks).mockResolvedValue(tracks([row(7, 'Liked one')]));
  vi.mocked(getPlaylistOrder).mockResolvedValue([]);
  vi.mocked(setPlaylistOrder).mockResolvedValue(undefined);
  vi.mocked(moveInPlaylist).mockResolvedValue(undefined);
  vi.mocked(removeFromPlaylist).mockResolvedValue(undefined);
  vi.mocked(addToPlaylist).mockResolvedValue(undefined);
  vi.mocked(subscribePlayback).mockImplementation(async (onStatus) =>
    onStatus(playbackStatus({})),
  );
  signIn(EDITING);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('Editing your playlists', () => {
  it('moves a song with Alt+↓, here and on Spotify, and focus follows it', async () => {
    const user = await openSidebar('Mine');
    expect(
      await screen.findByText(/Drag a song, or press Alt\+↑ or Alt\+↓/),
    ).toBeInTheDocument();
    const alpha = await screen.findByRole('button', { name: /^Alpha/ });
    alpha.focus();
    await user.keyboard('{Alt>}{ArrowDown}{/Alt}');
    expect(moveInPlaylist).toHaveBeenCalledWith(MINE.id, 0, 1);
    expect(songOrder()).toEqual(['Bravo', 'Alpha', 'Charlie']);
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /^Alpha/ })).toHaveFocus(),
    );
    // The top row can't go further up.
    screen.getByRole('button', { name: /^Bravo/ }).focus();
    await user.keyboard('{Alt>}{ArrowUp}{/Alt}');
    expect(moveInPlaylist).toHaveBeenCalledTimes(1);
  });

  it('drags a song to a new place without playing it', async () => {
    await openSidebar('Mine');
    const alpha = await screen.findByRole('button', { name: /^Alpha/ });
    // Rows 40 px tall, one under another.
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
      function (this: HTMLElement) {
        const i = Number(this.dataset.reorder ?? 0);
        return { top: i * 40, bottom: i * 40 + 40 } as DOMRect;
      },
    );
    const from = alpha.closest('tr')!;
    fireEvent.pointerDown(from, { button: 0, pointerId: 1, clientY: 20 });
    fireEvent.pointerMove(from, { pointerId: 1, clientY: 60 });
    expect(
      screen.getByRole('button', { name: /^Bravo/ }).closest('tr'),
    ).toHaveClass('drop-after');
    fireEvent.pointerMove(from, { pointerId: 1, clientY: 100 });
    fireEvent.pointerUp(from, { pointerId: 1, clientY: 100 });
    // The click that ends a drag lands on whichever row is under the pointer now.
    fireEvent.click(screen.getByRole('button', { name: /^Bravo/ }));
    expect(songOrder()).toEqual(['Bravo', 'Charlie', 'Alpha']);
    await waitFor(() =>
      expect(moveInPlaylist).toHaveBeenCalledWith(MINE.id, 0, 2),
    );
    expect(playFromList).not.toHaveBeenCalled();
    // The next click plays as usual.
    fireEvent.click(screen.getByRole('button', { name: /^Charlie/ }));
    expect(playFromList).toHaveBeenCalledTimes(1);
  });

  it('removes a song from its menu', async () => {
    const user = await openSidebar('Mine');
    await user.pointer({
      keys: '[MouseRight]',
      target: await screen.findByRole('button', { name: /^Bravo/ }),
    });
    const menu = screen.getByRole('menu', { name: 'Actions for Bravo' });
    expect(
      within(menu)
        .getAllByRole('menuitem')
        .map((item) => item.textContent),
    ).toEqual([
      'Add to queue',
      'Add to playlist…',
      'Move up',
      'Move down',
      'Remove from this playlist',
    ]);
    await user.click(
      within(menu).getByRole('menuitem', { name: 'Remove from this playlist' }),
    );
    expect(removeFromPlaylist).toHaveBeenCalledWith(MINE.id, [SONGS[1]!.uri]);
    expect(songOrder()).toEqual(['Alpha', 'Charlie']);
    expect(await screen.findByText('Removed from Mine')).toBeInTheDocument();
    expect(screen.getByText('2 tracks')).toBeInTheDocument();
  });

  it('puts the list back and says why when Spotify refuses', async () => {
    vi.mocked(moveInPlaylist).mockRejectedValue({
      kind: 'forbidden',
      hint: 'NotOwnerPlaylist',
    });
    const user = await openSidebar('Mine');
    (await screen.findByRole('button', { name: /^Alpha/ })).focus();
    await user.keyboard('{Alt>}{ArrowDown}{/Alt}');
    expect(
      await screen.findByText(
        'Spotify only lets you change playlists you own or collaborate on.',
      ),
    ).toBeInTheDocument();
    await waitFor(() => expect(playlistTracks).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(songOrder()).toEqual(['Alpha', 'Bravo', 'Charlie']),
    );
  });

  it('asks for permission to change playlists first', async () => {
    signIn(['SignIn', 'Playlists', 'PlaylistDetail']);
    vi.mocked(allowFeature).mockResolvedValue(status());
    const user = await openSidebar('Mine');
    (await screen.findByRole('button', { name: /^Alpha/ })).focus();
    await user.keyboard('{Alt>}{ArrowDown}{/Alt}');
    await waitFor(() =>
      expect(allowFeature).toHaveBeenCalledWith('PlaylistEdit'),
    );
    expect(moveInPlaylist).not.toHaveBeenCalled();
    await waitFor(() => expect(playlistTracks).toHaveBeenCalledTimes(2));
  });

  it('adds a song from any list to a playlist you can change', async () => {
    const user = await openSidebar('Liked Songs');
    await user.pointer({
      keys: '[MouseRight]',
      target: await screen.findByRole('button', { name: /^Liked one/ }),
    });
    await user.click(
      screen.getByRole('menuitem', { name: 'Add to playlist…' }),
    );
    const picker = screen.getByRole('menu', { name: 'Actions for Liked one' });
    const choices = within(picker).getAllByRole('menuitem');
    // Someone else's playlist isn't offered.
    expect(choices.map((item) => item.textContent)).toEqual([
      'New playlist…',
      'Mine',
      'Second',
    ]);
    expect(choices[0]).toHaveFocus();
    await user.click(choices[1]!);
    expect(addToPlaylist).toHaveBeenCalledWith(MINE.id, [row(7, '').uri]);
    expect(await screen.findByText('Added to Mine')).toBeInTheDocument();
    expect(await sidebarButton('Mine')).toHaveTextContent('4 tracks');
  });

  it('creates a private playlist from the sidebar and opens it', async () => {
    const made = { ...playlist(9, 'Road trip'), total: 0 };
    vi.mocked(createPlaylist).mockResolvedValue(made);
    vi.mocked(playlistTracks).mockResolvedValue(tracks([]));
    const user = userEvent.setup();
    render(<App />);
    await user.click(
      await screen.findByRole('button', { name: 'New playlist' }),
    );
    const form = screen.getByRole('form', { name: 'New playlist' });
    const name = within(form).getByLabelText('Playlist name');
    expect(name).toHaveFocus();
    expect(within(form).getByRole('button', { name: 'Create' })).toBeDisabled();
    await user.type(name, '  Road trip  {Enter}');
    expect(createPlaylist).toHaveBeenCalledWith('Road trip');
    expect(
      await screen.findByRole('heading', { name: 'Road trip' }),
    ).toBeInTheDocument();
    expect(sidebarOrder()[0]).toBe('Road trip');
    expect(screen.getByText('Created Road trip')).toBeInTheDocument();
    expect(
      screen.queryByRole('form', { name: 'New playlist' }),
    ).not.toBeInTheDocument();
  });

  it('creates a playlist with the song that asked for it', async () => {
    const made = { ...playlist(9, 'Road trip'), total: 0 };
    vi.mocked(createPlaylist).mockResolvedValue(made);
    const user = await openSidebar('Liked Songs');
    await user.pointer({
      keys: '[MouseRight]',
      target: await screen.findByRole('button', { name: /^Liked one/ }),
    });
    await user.click(
      screen.getByRole('menuitem', { name: 'Add to playlist…' }),
    );
    await user.click(screen.getByRole('menuitem', { name: 'New playlist…' }));
    const form = screen.getByRole('form', { name: 'New playlist' });
    expect(form).toHaveTextContent('“Liked one” goes in first.');
    await user.type(within(form).getByLabelText('Playlist name'), 'Road trip');
    await user.click(within(form).getByRole('button', { name: 'Create' }));
    await waitFor(() =>
      expect(addToPlaylist).toHaveBeenCalledWith(made.id, [row(7, '').uri]),
    );
    expect(await screen.findByText('Added to Road trip')).toBeInTheDocument();
  });

  it('closes the form on Escape without creating anything', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(
      await screen.findByRole('button', { name: 'New playlist' }),
    );
    await user.type(screen.getByLabelText('Playlist name'), 'Draft{Escape}');
    expect(
      screen.queryByRole('form', { name: 'New playlist' }),
    ).not.toBeInTheDocument();
    expect(createPlaylist).not.toHaveBeenCalled();
  });
});

describe('Melora’s own playlist order', () => {
  it('moves playlists with Alt+↑ and keeps the order, separate from Spotify', async () => {
    const user = userEvent.setup();
    render(<App />);
    await sidebarButton('Theirs');
    expect(sidebarOrder()).toEqual(['Mine', 'Second', 'Theirs']);
    (await sidebarButton('Theirs')).focus();
    await user.keyboard('{Alt>}{ArrowUp}{/Alt}');
    expect(setPlaylistOrder).toHaveBeenCalledWith([
      MINE.id,
      THEIRS.id,
      SECOND.id,
    ]);
    expect(sidebarOrder()).toEqual(['Mine', 'Theirs', 'Second']);
    expect(await sidebarButton('Theirs')).toHaveFocus();
    // Spotify's own order isn't touched.
    expect(moveInPlaylist).not.toHaveBeenCalled();

    expect(screen.getByText(/In Melora’s order/)).toBeInTheDocument();
    await user.click(
      screen.getByRole('button', { name: 'Use Spotify’s order' }),
    );
    expect(setPlaylistOrder).toHaveBeenLastCalledWith([]);
    expect(sidebarOrder()).toEqual(['Mine', 'Second', 'Theirs']);
  });

  it('opens in the saved order, with new playlists after it', async () => {
    vi.mocked(getPlaylistOrder).mockResolvedValue([SECOND.id]);
    render(<App />);
    await sidebarButton('Theirs');
    await waitFor(() =>
      expect(sidebarOrder()).toEqual(['Second', 'Mine', 'Theirs']),
    );
  });

  it('moves a playlist from its menu', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.pointer({
      keys: '[MouseRight]',
      target: await sidebarButton('Mine'),
    });
    const menu = screen.getByRole('menu', { name: 'Actions for Mine' });
    expect(
      within(menu).getByRole('menuitem', { name: 'Move up' }),
    ).toBeDisabled();
    // The first item that can be used has focus.
    expect(
      within(menu).getByRole('menuitem', { name: 'Move down' }),
    ).toHaveFocus();
    await user.click(within(menu).getByRole('menuitem', { name: 'Move down' }));
    expect(setPlaylistOrder).toHaveBeenCalledWith([
      SECOND.id,
      MINE.id,
      THEIRS.id,
    ]);
  });

  it('says so when the order can’t be saved', async () => {
    vi.mocked(setPlaylistOrder).mockRejectedValue({
      kind: 'settingsUnwritable',
    });
    const user = userEvent.setup();
    render(<App />);
    (await sidebarButton('Second')).focus();
    await user.keyboard('{Alt>}{ArrowUp}{/Alt}');
    expect(
      await screen.findByText(/Melora couldn’t save its playlist order/),
    ).toBeInTheDocument();
  });
});
