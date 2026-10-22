// Shared Web Playback SDK bridge for every ora client. It checks what the webview supports,
// loads Spotify's SDK only when some DRM system is available, forwards the SDK's events to the
// `ora` plugin and runs the player commands Rust sends back over a channel.
//
// The access token is requested from the plugin each time the SDK asks for one and handed
// straight to the SDK's callback. It is never stored, logged or reported here.

const SDK_URL = 'https://sdk.scdn.co/spotify-player.js';
const KEY_SYSTEMS = {
  widevine: 'com.widevine.alpha',
  playready: 'com.microsoft.playready',
  fairplay: 'com.apple.fps',
};
const MEDIA_CONFIG = [
  {
    initDataTypes: ['cenc'],
    audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }],
  },
];
const SDK_ERRORS = {
  initialization_error: 'initialization',
  authentication_error: 'authentication',
  account_error: 'account',
  playback_error: 'playback',
};

/**
 * @param {{
 *   invoke: (command: string, args?: Record<string, unknown>) => Promise<any>,
 *   Channel: new () => { onmessage: (message: any) => void },
 *   name: string,
 *   volume?: number,
 *   window?: Window & typeof globalThis,
 * }} options
 */
export function install(options) {
  const { invoke, Channel, name } = options;
  const win = options.window ?? window;
  let volume = options.volume ?? 0.7;
  let player = null;
  let enabling = null;
  let loading = null;

  const report = (event) =>
    invoke('plugin:ora|sdk_event', { event }).catch(() => undefined);

  // Every CSP block on the page is a finding for the spike: only the directive and the
  // blocked origin are reported (Rust reduces it again).
  win.document.addEventListener('securitypolicyviolation', (event) => {
    void report({
      type: 'cspViolation',
      directive: String(event.effectiveDirective || event.violatedDirective),
      origin: String(event.blockedURI),
    });
  });
  win.addEventListener('pagehide', () => disconnect());

  async function supports(keySystem) {
    if (typeof win.navigator.requestMediaKeySystemAccess !== 'function') {
      return false;
    }
    try {
      await win.navigator.requestMediaKeySystemAccess(keySystem, MEDIA_CONFIG);
      return true;
    } catch {
      return false;
    }
  }

  async function probe() {
    const [widevine, playready, fairplay] = await Promise.all([
      supports(KEY_SYSTEMS.widevine),
      supports(KEY_SYSTEMS.playready),
      supports(KEY_SYSTEMS.fairplay),
    ]);
    let platformVersion = null;
    let webviewVersion = null;
    try {
      const data = await win.navigator.userAgentData?.getHighEntropyValues([
        'platformVersion',
        'fullVersionList',
      ]);
      platformVersion = data?.platformVersion || null;
      const brands = data?.fullVersionList ?? [];
      const engine =
        brands.find((b) => /WebView2/i.test(b.brand)) ??
        brands.find((b) => /Edge/i.test(b.brand)) ??
        brands.find((b) => /Chromium/i.test(b.brand));
      webviewVersion = engine ? `${engine.brand} ${engine.version}` : null;
    } catch {
      // Not every engine has userAgentData; the user agent string still says enough.
    }
    return {
      secure: Boolean(win.isSecureContext),
      widevine,
      playready,
      fairplay,
      userAgent: String(win.navigator.userAgent),
      platformVersion,
      webviewVersion,
    };
  }

  function loadSdk() {
    if (win.Spotify?.Player) return Promise.resolve(win.Spotify);
    loading ??= new Promise((resolve, reject) => {
      // The SDK calls this when it has loaded; it must exist before the script runs.
      win.onSpotifyWebPlaybackSDKReady = () => resolve(win.Spotify);
      const script = win.document.createElement('script');
      script.src = SDK_URL;
      script.async = true;
      script.onerror = () => {
        loading = null;
        script.remove();
        reject(new Error('The Spotify player script could not be loaded.'));
      };
      win.document.head.append(script);
    });
    return loading;
  }

  function reduce(state) {
    if (!state) return null;
    const track = state.track_window?.current_track;
    const whole = (value) => Math.max(0, Math.round(Number(value) || 0));
    return {
      paused: Boolean(state.paused),
      positionMs: whole(state.position),
      durationMs: whole(state.duration),
      track: track
        ? {
            uri: String(track.uri ?? ''),
            name: String(track.name ?? ''),
            artists: (track.artists ?? []).map((a) => String(a?.name ?? '')),
            album: String(track.album?.name ?? ''),
          }
        : null,
    };
  }

  function run(command) {
    if (!player) return;
    switch (command?.op) {
      case 'togglePlay':
        void player.togglePlay();
        break;
      case 'seek':
        void player.seek(command.positionMs);
        break;
      case 'next':
        void player.nextTrack();
        break;
      case 'previous':
        void player.previousTrack();
        break;
      case 'setVolume':
        volume = command.volume;
        void player.setVolume(command.volume);
        break;
      case 'disconnect':
        disconnect();
        break;
      default:
        break;
    }
  }

  function connectPlayer(Spotify) {
    const created = new Spotify.Player({
      name,
      volume,
      enableMediaSession: true,
      getOAuthToken: (callback) => {
        invoke('plugin:ora|access_token').then(callback, (error) => {
          // Without a token the SDK would wait forever: stop, and say why.
          void report({
            type: 'error',
            error: 'authentication',
            message: `No access token (${String(error?.kind ?? 'unknown')}).`,
          });
          disconnect();
        });
      },
    });
    created.addListener('ready', ({ device_id }) => {
      void report({ type: 'ready', deviceId: String(device_id) });
    });
    created.addListener('not_ready', ({ device_id }) => {
      void report({ type: 'notReady', deviceId: String(device_id) });
    });
    created.addListener('player_state_changed', (state) => {
      void report({ type: 'state', state: reduce(state) });
    });
    created.addListener('autoplay_failed', () => {
      void report({
        type: 'error',
        error: 'autoplay',
        message: 'Autoplay was blocked.',
      });
    });
    for (const [event, kind] of Object.entries(SDK_ERRORS)) {
      created.addListener(event, ({ message }) => {
        void report({ type: 'error', error: kind, message: String(message) });
      });
    }
    return created;
  }

  /** Probes, loads the SDK and connects. Resolves whether this app became a device. */
  function enable() {
    enabling ??= (async () => {
      const result = await probe();
      await report({ type: 'probe', probe: result });
      const drm = result.widevine || result.playready || result.fairplay;
      if (!result.secure || !drm) return false;
      const Spotify = await loadSdk();
      const channel = new Channel();
      channel.onmessage = run;
      await invoke('plugin:ora|sdk_attach', { onCommand: channel });
      player = connectPlayer(Spotify);
      const connected = await player.connect();
      if (!connected) {
        await report({
          type: 'error',
          error: 'initialization',
          message: 'The player could not connect to Spotify.',
        });
      }
      return connected;
    })().catch(async (error) => {
      await report({
        type: 'error',
        error: 'initialization',
        message: String(error?.message ?? error),
      });
      return false;
    });
    const attempt = enabling;
    void attempt.then((connected) => {
      if (!connected && enabling === attempt) {
        disconnect();
      }
    });
    return attempt;
  }

  /** Call synchronously from a click, before the first transfer or play. */
  function activate() {
    void player?.activateElement?.();
  }

  function disconnect() {
    player?.disconnect();
    player = null;
    enabling = null;
  }

  return { enable, activate, disconnect };
}
