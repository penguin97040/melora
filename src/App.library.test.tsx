import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import {
  activatePlayback,
  allowFeature,
  getAppInfo,
  getWindowChrome,
  likedTracks,
  listPlaylists,
  openInSpotify,
  playFromList,
  playlistTracks,
  savePreferences,
  showMainWindow,
  subscribeSpotify,
} from './native/client';
import type {
  PlaylistSummary,
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
  playlistTracks: vi.fn(),
  likedTracks: vi.fn(),
  playFromList: vi.fn(),
}));

const LISTS: Feature[] = [
  'SignIn',
  'Playlists',
  'PlaylistDetail',
  'SavedTracks',
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

function playlist(id: string, name: string, readable = true): PlaylistSummary {
  return {
    id,
    name,
    owner: 'Kiri',
    total: 3,
    imageUrl: null,
    spotifyUrl: `https://open.spotify.com/playlist/${id}`,
    readable,
  };
}

const MINE = playlist('PlAyLiSt0123456789abcd', 'Road trip');
const THEIRS = playlist('OtHeRs0123456789abcdef', 'Top hits', false);

function row(position: number, name: string, uri: string | null): TrackRow {
  const id = uri?.split(':')[2];
  return {
    position,
    uri,
    name,
    artists: ['Artist A', 'Artist B'],
    album: 'Album',
    releaseDate: '',
    durationMs: 3_725_000,
    spotifyUrl: id ? `https://open.spotify.com/track/${id}` : null,
  };
}

const TRACK = (n: number) => `spotify:track:Track${n}0123456789abcdefg`;

function page(items: TrackRow[], nextOffset: number | null = null): TrackPage {
  return { items, total: 60, nextOffset };
}

function signIn(granted: Feature[]) {
  vi.mocked(subscribeSpotify).mockImplementation(async (onStatus) =>
    onStatus(signedIn(granted)),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getWindowChrome).mockResolvedValue('preview');
  vi.mocked(getAppInfo).mockResolvedValue({
    version: '0.5.2',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
  vi.mocked(showMainWindow).mockResolvedValue(undefined);
  vi.mocked(playFromList).mockResolvedValue(undefined);
  vi.mocked(openInSpotify).mockResolvedValue(undefined);
  vi.mocked(listPlaylists).mockResolvedValue({
    items: [MINE, THEIRS],
    total: 2,
    nextOffset: null,
  });
  signIn(LISTS);
});

describe('Spotify playlists and Liked Songs', () => {
  it('keeps the preview until playlists are allowed, then asks for them', async () => {
    signIn(['SignIn', 'Profile']);
    vi.mocked(allowFeature).mockResolvedValue(signedIn(LISTS));
    render(<App />);
    const preview = screen.getByRole('region', { name: 'Preview playlists' });
    await userEvent.setup().click(
      await within(preview).findByRole('button', {
        name: 'Show my Spotify playlists',
      }),
    );
    expect(allowFeature).toHaveBeenCalledWith('Playlists');
    expect(listPlaylists).not.toHaveBeenCalled();
  });

  it('lists playlists and plays a playlist from the clicked row', async () => {
    vi.mocked(playlistTracks).mockResolvedValue(
      page([
        row(0, 'First', TRACK(1)),
        row(1, 'Home recording', null),
        row(2, 'Third', TRACK(3)),
      ]),
    );
    const user = userEvent.setup();
    render(<App />);
    const sidebar = await screen.findByRole('region', {
      name: 'Your Spotify playlists',
    });
    expect(within(sidebar).getByText('06')).toBeInTheDocument();
    expect(within(sidebar).getAllByText('Kiri · 3 tracks')).toHaveLength(2);
    await user.click(
      within(sidebar).getByRole('button', { name: /Road trip/ }),
    );
    expect(
      await screen.findByRole('heading', { name: 'Road trip' }),
    ).toBeInTheDocument();
    expect(playlistTracks).toHaveBeenCalledWith(MINE.id, 0);
    expect(screen.getAllByText('From Spotify').length).toBeGreaterThan(0);
    expect(screen.getAllByText('1:02:05')).toHaveLength(3);

    // Local files stay listed but can't be played.
    expect(
      screen.getByRole('button', { name: /^Home recording/ }),
    ).toBeDisabled();
    await user.click(screen.getByRole('button', { name: /^Third/ }));
    expect(activatePlayback).toHaveBeenCalled();
    expect(playFromList).toHaveBeenCalledWith({
      kind: 'context',
      uri: `spotify:playlist:${MINE.id}`,
      position: 2,
    });

    await user.click(screen.getByRole('button', { name: 'Open in Spotify' }));
    expect(openInSpotify).toHaveBeenCalledWith(MINE.spotifyUrl);
  });

  it('plays Liked Songs from the clicked track onwards and loads more on request', async () => {
    vi.mocked(likedTracks)
      .mockResolvedValueOnce(
        page([row(0, 'Newest', TRACK(1)), row(1, 'Older', TRACK(2))], 50),
      )
      .mockResolvedValueOnce(page([row(50, 'Oldest', TRACK(3))]));
    const user = userEvent.setup();
    render(<App />);
    await user.click(
      await screen.findByRole('button', { name: /Liked Songs/ }),
    );
    await user.click(
      await screen.findByRole('button', { name: 'Show more tracks' }),
    );
    expect(likedTracks).toHaveBeenLastCalledWith(50);
    await user.click(await screen.findByRole('button', { name: /^Older/ }));
    expect(playFromList).toHaveBeenCalledWith({
      kind: 'items',
      uris: [TRACK(2), TRACK(3)],
    });
    expect(screen.getByRole('button', { name: /^Newest/ })).toBeEnabled();
  });

  it('explains playlists Spotify won’t list, and still plays them', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole('button', { name: /Top hits/ }));
    expect(
      screen.getByText(/only shows the tracks of playlists you own/),
    ).toBeInTheDocument();
    expect(playlistTracks).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Play playlist' }));
    expect(playFromList).toHaveBeenCalledWith({
      kind: 'context',
      uri: `spotify:playlist:${THEIRS.id}`,
      position: null,
    });
  });

  it('asks for a missing permission, then loads again', async () => {
    signIn(['SignIn', 'Playlists', 'PlaylistDetail']);
    vi.mocked(likedTracks)
      .mockRejectedValueOnce({ kind: 'needsScope', feature: 'SavedTracks' })
      .mockResolvedValueOnce(page([row(0, 'Newest', TRACK(1))]));
    vi.mocked(allowFeature).mockResolvedValue(signedIn(LISTS));
    const user = userEvent.setup();
    render(<App />);
    await user.click(
      await screen.findByRole('button', { name: /Liked Songs/ }),
    );
    expect(
      await screen.findByText(/additional permission for “Liked Songs”/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Allow' }));
    expect(allowFeature).toHaveBeenCalledWith('SavedTracks');
    expect(
      await screen.findByRole('button', { name: /^Newest/ }),
    ).toBeInTheDocument();
  });

  it('says when no device can play, and clears it after a later success', async () => {
    vi.mocked(playlistTracks).mockResolvedValue(
      page([row(0, 'First', TRACK(1))]),
    );
    vi.mocked(playFromList)
      .mockRejectedValueOnce({ kind: 'noActiveDevice' })
      .mockResolvedValueOnce(undefined);
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole('button', { name: /Road trip/ }));
    await user.click(await screen.findByRole('button', { name: /^First/ }));
    expect(
      await screen.findByText(/No Spotify device is active/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /^First/ }));
    await waitFor(() =>
      expect(
        screen.queryByText(/No Spotify device is active/),
      ).not.toBeInTheDocument(),
    );
  });
});
