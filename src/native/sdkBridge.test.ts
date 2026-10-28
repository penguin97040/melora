import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { install } from '../../ora/sdk-bridge/sdk-bridge.js';

type Listener = (payload: unknown) => void;
type Options = {
  name: string;
  enableMediaSession: boolean;
  getOAuthToken: (callback: (token: string) => void) => void;
};

class FakePlayer {
  static created: FakePlayer[] = [];
  /** Makes `connect()` never settle, like a stalled network. */
  static hang = false;
  listeners = new Map<string, Listener>();
  connect = vi.fn(() =>
    FakePlayer.hang
      ? new Promise<boolean>(() => undefined)
      : Promise.resolve(true),
  );
  disconnect = vi.fn();
  activateElement = vi.fn(async () => undefined);
  togglePlay = vi.fn(async () => undefined);
  seek = vi.fn(async () => undefined);
  nextTrack = vi.fn(async () => undefined);
  previousTrack = vi.fn(async () => undefined);
  setVolume = vi.fn(async () => undefined);
  constructor(public options: Options) {
    FakePlayer.created.push(this);
  }
  addListener(event: string, listener: Listener) {
    this.listeners.set(event, listener);
  }
  emit(event: string, payload: unknown) {
    this.listeners.get(event)?.(payload);
  }
}

class FakeChannel {
  static created: FakeChannel[] = [];
  onmessage: (message: never) => void = () => undefined;
  constructor() {
    FakeChannel.created.push(this);
  }
}

type SdkWindow = Window &
  typeof globalThis & {
    Spotify?: { Player: typeof FakePlayer };
    onSpotifyWebPlaybackSDKReady?: () => void;
  };
const sdkWindow = window as SdkWindow;

const calls: { command: string; args?: Record<string, unknown> }[] = [];
let tokens = 0;
let tokenFails = false;
const invoke = vi.fn(
  async (command: string, args?: Record<string, unknown>) => {
    calls.push(args === undefined ? { command } : { command, args });
    if (command === 'plugin:ora|access_token') {
      if (tokenFails) throw { kind: 'needsScope' };
      tokens += 1;
      return `fixture-${tokens}`;
    }
    return undefined;
  },
) as unknown as <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

function events(type?: string): Record<string, unknown>[] {
  return calls
    .filter((c) => c.command === 'plugin:ora|sdk_event')
    .map((c) => c.args?.['event'] as Record<string, unknown>)
    .filter((e) => type === undefined || e['type'] === type);
}

function sdkScripts(): HTMLScriptElement[] {
  return [...document.querySelectorAll('script')].filter((s) =>
    s.src.startsWith('https://sdk.scdn.co/'),
  );
}

function setDrm(available: boolean) {
  Object.defineProperty(window, 'isSecureContext', {
    value: true,
    configurable: true,
  });
  Object.defineProperty(navigator, 'requestMediaKeySystemAccess', {
    value: vi.fn((keySystem: string) =>
      available && keySystem === 'com.widevine.alpha'
        ? Promise.resolve({})
        : Promise.reject(new Error('unsupported')),
    ),
    configurable: true,
  });
}

/** Lets the bridge insert its script, then plays the SDK's part: define Spotify, call ready. */
async function sdkLoads() {
  await vi.waitFor(() => expect(sdkScripts()).toHaveLength(1));
  sdkWindow.Spotify = { Player: FakePlayer };
  sdkWindow.onSpotifyWebPlaybackSDKReady?.();
}

beforeEach(() => {
  calls.length = 0;
  tokens = 0;
  tokenFails = false;
  FakePlayer.created = [];
  FakePlayer.hang = false;
  FakeChannel.created = [];
  delete sdkWindow.Spotify;
  delete sdkWindow.onSpotifyWebPlaybackSDKReady;
});

afterEach(() => {
  for (const script of sdkScripts()) script.remove();
});

function bridge(timeoutMs?: number) {
  return install({
    invoke,
    Channel: FakeChannel,
    name: 'Melora',
    volume: 0.5,
    ...(timeoutMs === undefined ? {} : { timeoutMs }),
  });
}

describe('sdk-bridge', () => {
  it('reports the probe and never loads the SDK without DRM', async () => {
    setDrm(false);
    await expect(bridge().enable()).resolves.toBe(false);
    const [probe] = events('probe');
    expect(probe?.['probe']).toMatchObject({
      secure: true,
      widevine: false,
      playready: false,
    });
    expect(sdkScripts()).toHaveLength(0);
    expect(calls.some((c) => c.command === 'plugin:ora|sdk_attach')).toBe(
      false,
    );
  });

  it('loads the SDK once, connects a Melora device and forwards its events', async () => {
    setDrm(true);
    const playback = bridge();
    const first = playback.enable();
    const second = playback.enable();
    await sdkLoads();
    await expect(first).resolves.toBe(true);
    await expect(second).resolves.toBe(true);

    expect(sdkScripts()).toHaveLength(1);
    expect(FakePlayer.created).toHaveLength(1);
    const player = FakePlayer.created[0]!;
    expect(player.options.name).toBe('Melora');
    expect(player.options.enableMediaSession).toBe(false);
    expect(player.connect).toHaveBeenCalledTimes(1);
    expect(
      calls.filter((c) => c.command === 'plugin:ora|sdk_attach'),
    ).toHaveLength(1);

    player.emit('ready', { device_id: 'device1' });
    player.emit('player_state_changed', {
      paused: false,
      position: 1200.4,
      shuffle: true,
      repeat_mode: 1,
      disallows: { seeking: true, skipping_prev: false },
      track_window: {
        current_track: {
          uri: 'spotify:track:x',
          name: 'Song',
          duration_ms: 180000,
          artists: [{ name: 'Artist' }],
          album: {
            name: 'Album',
            images: [
              { url: 'https://i.scdn.co/image/large', width: 640 },
              { url: 'https://i.scdn.co/image/medium', width: 300 },
              { url: 'https://i.scdn.co/image/small', width: 64 },
            ],
          },
        },
      },
    });
    player.emit('player_state_changed', null);
    player.emit('account_error', { message: 'Premium required' });
    await vi.waitFor(() => expect(events('error')).toHaveLength(1));
    expect(events('ready')).toEqual([{ type: 'ready', deviceId: 'device1' }]);
    expect(events('state')).toEqual([
      {
        type: 'state',
        state: {
          paused: false,
          positionMs: 1200,
          durationMs: 180000,
          track: {
            uri: 'spotify:track:x',
            name: 'Song',
            artists: ['Artist'],
            album: 'Album',
            imageUrl: 'https://i.scdn.co/image/medium',
          },
          shuffle: true,
          repeat: 'context',
          disallows: {
            pause: false,
            resume: false,
            next: false,
            previous: false,
            seek: true,
          },
        },
      },
      { type: 'state', state: null },
    ]);
    expect(events('error')).toEqual([
      { type: 'error', error: 'account', message: 'Premium required' },
    ]);
  });

  it('asks for a token every time and keeps none of them', async () => {
    setDrm(true);
    const playback = bridge();
    const enabled = playback.enable();
    await sdkLoads();
    await enabled;
    const player = FakePlayer.created[0]!;
    const received: string[] = [];
    player.options.getOAuthToken((token) => received.push(token));
    player.options.getOAuthToken((token) => received.push(token));
    await vi.waitFor(() => expect(received).toHaveLength(2));
    expect(received).toEqual(['fixture-1', 'fixture-2']);
    // No later report or call carries a token.
    expect(JSON.stringify(calls)).not.toContain('fixture-');
  });

  it('stops and says why when no token is available', async () => {
    setDrm(true);
    const playback = bridge();
    const enabled = playback.enable();
    await sdkLoads();
    await enabled;
    tokenFails = true;
    const player = FakePlayer.created[0]!;
    const callback = vi.fn();
    player.options.getOAuthToken(callback);
    await vi.waitFor(() => expect(player.disconnect).toHaveBeenCalled());
    expect(callback).not.toHaveBeenCalled();
    expect(events('error')[0]).toMatchObject({
      error: 'authentication',
      message: 'No access token (needsScope).',
    });
    // Rust hears that the device is gone, so its buttons and media keys stop claiming it.
    await vi.waitFor(() =>
      expect(events('disconnected')).toEqual([{ type: 'disconnected' }]),
    );
  });

  it('starts afresh after an initialisation error and says the device is gone', async () => {
    setDrm(true);
    const playback = bridge();
    const enabled = playback.enable();
    await sdkLoads();
    await expect(enabled).resolves.toBe(true);
    const first = FakePlayer.created[0]!;

    first.emit('initialization_error', { message: 'Failed to initialise' });
    expect(first.disconnect).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => expect(events('disconnected')).toHaveLength(1));

    await expect(playback.enable()).resolves.toBe(true);
    expect(FakePlayer.created).toHaveLength(2);
  });

  it('reports a disconnect when the page goes away', async () => {
    setDrm(true);
    const playback = bridge();
    const enabled = playback.enable();
    await sdkLoads();
    await enabled;
    window.dispatchEvent(new Event('pagehide'));
    expect(FakePlayer.created[0]!.disconnect).toHaveBeenCalled();
    await vi.waitFor(() =>
      expect(events('disconnected')).toContainEqual({ type: 'disconnected' }),
    );
  });

  it('gives up on an SDK that never loads, and loads it again next time', async () => {
    setDrm(true);
    const playback = bridge(300);
    await expect(playback.enable()).resolves.toBe(false);
    expect(events('error')).toEqual([
      {
        type: 'error',
        error: 'initialization',
        message: 'The Spotify player took too long to load.',
      },
    ]);
    for (const script of sdkScripts()) script.remove();

    const again = playback.enable();
    await sdkLoads();
    await expect(again).resolves.toBe(true);
  });

  it('gives up on a connection that never settles', async () => {
    setDrm(true);
    FakePlayer.hang = true;
    const playback = bridge(300);
    const enabled = playback.enable();
    await sdkLoads();
    await expect(enabled).resolves.toBe(false);
    expect(events('error')).toEqual([
      {
        type: 'error',
        error: 'initialization',
        message: 'The player took too long to connect to Spotify.',
      },
    ]);
    expect(FakePlayer.created[0]!.disconnect).toHaveBeenCalled();
    await vi.waitFor(() => expect(events('disconnected')).toHaveLength(1));

    FakePlayer.hang = false;
    await expect(playback.enable()).resolves.toBe(true);
  });

  it('runs commands from Rust and activates synchronously', async () => {
    setDrm(true);
    const playback = bridge();
    const enabled = playback.enable();
    await sdkLoads();
    await enabled;
    const player = FakePlayer.created[0]!;
    const channel = FakeChannel.created[0]!;
    const send = channel.onmessage as (message: unknown) => void;

    playback.activate();
    expect(player.activateElement).toHaveBeenCalledTimes(1);
    send({ op: 'seek', positionMs: 5000 });
    send({ op: 'setVolume', volume: 0.25 });
    send({ op: 'togglePlay' });
    send({ op: 'unknown' });
    expect(player.seek).toHaveBeenCalledWith(5000);
    expect(player.setVolume).toHaveBeenCalledWith(0.25);
    expect(player.togglePlay).toHaveBeenCalledTimes(1);
    send({ op: 'disconnect' });
    expect(player.disconnect).toHaveBeenCalledTimes(1);
    // Rust asked for it, so the bridge doesn't report it back.
    await Promise.resolve();
    expect(events('disconnected')).toHaveLength(0);
  });
});
