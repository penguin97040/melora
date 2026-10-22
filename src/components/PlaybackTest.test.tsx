import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PlaybackTest } from './PlaybackTest';
import { describeError } from '../features/account';
import { diagnosticsLine, formatTime, positionAt } from '../features/playback';
import {
  activatePlayback,
  allowPlayback,
  enablePlayback,
  playHere,
  sendPlaybackCommand,
} from '../native/client';
import type { PlaybackStatus, SpotifyStatus } from '../native/client';
import type { Feature } from '../native/bindings/Feature';

vi.mock('../native/client', () => ({
  hasNativeStorage: () => true,
  activatePlayback: vi.fn(),
  allowPlayback: vi.fn(),
  enablePlayback: vi.fn(),
  playHere: vi.fn(),
  sendPlaybackCommand: vi.fn(),
  openInSpotify: vi.fn(),
}));

function spotify(granted: Feature[]): SpotifyStatus {
  return {
    auth: { kind: 'signedIn', profile: null, sessionOnly: false, granted },
    clientIdFromEnvironment: false,
    clientIdHint: null,
    lastError: null,
    redirectUri: 'http://127.0.0.1:43821/callback',
  };
}

function playback(extra: Partial<PlaybackStatus> = {}): PlaybackStatus {
  return {
    tier: 'off',
    nowPlaying: null,
    paused: true,
    positionMs: 0,
    durationMs: 0,
    updatedAtMs: 0,
    issue: null,
    probe: null,
    cspBlocks: [],
    ...extra,
  };
}

const allowed: Feature[] = ['SignIn', 'Profile', 'EmbeddedPlayback'];

beforeEach(() => {
  vi.clearAllMocks();
});

describe('Playback in Melora (test)', () => {
  it('asks for Spotify’s permission first', async () => {
    vi.mocked(allowPlayback).mockResolvedValue(spotify(allowed));
    render(
      <PlaybackTest
        spotify={spotify(['SignIn', 'Profile'])}
        playback={playback()}
        volume={70}
      />,
    );
    expect(
      screen.queryByRole('button', { name: 'Enable playback in Melora' }),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole('button', { name: 'Allow playback' }),
    );
    expect(allowPlayback).toHaveBeenCalledTimes(1);
  });

  it('enables the player with the preferred volume', async () => {
    vi.mocked(enablePlayback).mockResolvedValue(true);
    render(
      <PlaybackTest
        spotify={spotify(allowed)}
        playback={playback()}
        volume={70}
      />,
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Enable playback in Melora' }),
    );
    expect(enablePlayback).toHaveBeenCalledWith(0.7);
  });

  it('activates inside the click, then plays the pasted link here', async () => {
    const order: string[] = [];
    vi.mocked(activatePlayback).mockImplementation(() => {
      order.push('activate');
    });
    vi.mocked(playHere).mockImplementation(async () => {
      order.push('play');
    });
    render(
      <PlaybackTest
        spotify={spotify(allowed)}
        playback={playback({ tier: 'ready' })}
        volume={70}
      />,
    );
    await userEvent.type(
      screen.getByLabelText(/Spotify link/),
      'https://open.spotify.com/album/x',
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Play in Melora' }),
    );
    expect(order).toEqual(['activate', 'play']);
    expect(playHere).toHaveBeenCalledWith('https://open.spotify.com/album/x');
  });

  it('shows what is playing and sends controls through Rust', async () => {
    vi.mocked(sendPlaybackCommand).mockResolvedValue();
    render(
      <PlaybackTest
        spotify={spotify(allowed)}
        playback={playback({
          tier: 'active',
          paused: true,
          positionMs: 20_000,
          durationMs: 180_000,
          nowPlaying: {
            name: 'Song',
            artists: ['Artist'],
            album: 'Album',
            spotifyUrl: 'https://open.spotify.com/track/x',
          },
        })}
        volume={70}
      />,
    );
    expect(screen.getByText(/Song — Artist \(Album\)/)).toHaveTextContent(
      '0:20 / 3:00',
    );
    await userEvent.click(screen.getByRole('button', { name: 'Play' }));
    await userEvent.click(screen.getByRole('button', { name: 'Back 15 s' }));
    expect(sendPlaybackCommand).toHaveBeenCalledWith({ op: 'togglePlay' });
    expect(sendPlaybackCommand).toHaveBeenCalledWith({
      op: 'seek',
      positionMs: 5_000,
    });
  });

  it('explains an unsupported engine and SDK errors', () => {
    render(
      <PlaybackTest
        spotify={spotify(allowed)}
        playback={playback({
          tier: 'unsupported',
          issue: { kind: 'initialization', message: 'No EME' },
          probe: {
            secure: true,
            widevine: false,
            playready: false,
            fairplay: false,
            userAgent: 'test',
            platformVersion: null,
            webviewVersion: null,
          },
        })}
        volume={70}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent(
      'isn’t available on this system',
    );
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Spotify said: “No EME”',
    );
    expect(screen.getByText(/Widevine no/)).toBeInTheDocument();
  });
});

describe('playback helpers', () => {
  it('interpolates progress only while playing', () => {
    const status = playback({
      tier: 'active',
      paused: false,
      positionMs: 1_000,
      durationMs: 5_000,
      updatedAtMs: 10_000,
    });
    expect(positionAt(status, 12_500)).toBe(3_500);
    expect(positionAt(status, 99_000)).toBe(5_000);
    expect(positionAt(status, 9_000)).toBe(1_000);
    expect(positionAt({ ...status, paused: true }, 12_500)).toBe(1_000);
    expect(formatTime(65_900)).toBe('1:05');
  });

  it('lists CSP blocks in the diagnostics', () => {
    const line = diagnosticsLine(
      playback({
        probe: {
          secure: true,
          widevine: true,
          playready: true,
          fairplay: false,
          userAgent: 'test',
          platformVersion: '19.0.0',
          webviewVersion: 'Microsoft Edge WebView2 141.0',
        },
        cspBlocks: ['script-src https://cdn.example'],
      }),
    );
    expect(line).toContain('engine Microsoft Edge WebView2 141.0');
    expect(line).toContain('CSP blocks script-src https://cdn.example');
  });

  it('names the feature that needs permission and the link format', () => {
    expect(
      describeError({ kind: 'needsScope', feature: 'EmbeddedPlayback' }),
    ).toContain('“Playback in Melora”');
    expect(describeError({ kind: 'invalidSpotifyLink' })).toContain(
      'Share → Copy link',
    );
    expect(describeError({ kind: 'forbidden', hint: 'Premium' })).toContain(
      'Premium',
    );
  });
});
