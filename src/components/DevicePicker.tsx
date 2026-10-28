import { useEffect, useRef, useState } from 'react';
import type { CSSProperties, KeyboardEvent } from 'react';
import {
  activatePlayback,
  allowDeviceControl,
  listDevices,
  playHere,
  refreshPlayback,
  transferPlayback,
} from '../native/client';
import type {
  Device,
  PlaybackStatus,
  PlayerCommand,
  SpotifyStatus,
} from '../native/client';
import { describeFailure } from '../features/account';
import {
  deviceControlAllowed,
  deviceKindLabel,
  deviceName,
  isLive,
  isRemote,
} from '../features/playback';

/** How often the open list is read again (DO_NEXT: every 10–30 s while the picker is open). */
export const DEVICE_REFRESH_MS = 15_000;

/**
 * Beside the player, bottom-aligned with the device line, so it never covers the artwork. In
 * layouts too narrow for that, below the device line instead.
 */
function placeBeside(slot: HTMLElement): CSSProperties {
  const gap = 8;
  const line = slot.getBoundingClientRect();
  const player = slot.closest('.player')?.getBoundingClientRect() ?? line;
  const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
  if (player.left >= 18 * rem + 2 * gap) {
    return {
      right: window.innerWidth - player.left + gap,
      bottom: Math.max(gap, window.innerHeight - line.bottom),
      maxHeight: Math.max(0, line.bottom - gap),
    };
  }
  return {
    left: Math.max(gap, line.left),
    top: line.bottom + gap,
    maxHeight: Math.max(120, window.innerHeight - line.bottom - 2 * gap),
  };
}

// The Playback device line in the player, and the list of the account's Spotify devices.
export function DevicePicker({
  spotify,
  playback,
  volume,
  onVolume,
  onVolumeCommit,
  onCommand,
  onOpenSettings,
}: {
  spotify: SpotifyStatus | null;
  playback: PlaybackStatus | null;
  /** Melora's own volume, 0–100 (the Settings preference). */
  volume: number;
  /** While the slider moves. */
  onVolume: (volume: number) => void;
  /** When it's let go: saved and sent, once. */
  onVolumeCommit: (volume: number) => void;
  onCommand: (command: PlayerCommand) => void;
  onOpenSettings: () => void;
}) {
  const [placement, setPlacement] = useState<CSSProperties | null>(null);
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The remote volume while it's being dragged; it's sent on release only.
  const [dragging, setDragging] = useState<number | null>(null);
  const slotRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const open = placement !== null;
  const signedIn = spotify?.auth.kind === 'signedIn';
  const allowed = deviceControlAllowed(spotify);
  const tier = playback?.tier ?? 'off';
  const sdkReady = tier === 'ready' || tier === 'active';

  useEffect(() => {
    if (!open || !allowed) return;
    let active = true;
    const load = () =>
      void listDevices()
        .then((list) => {
          if (!active) return;
          setDevices(list);
          setMessage(null);
        })
        .catch((error: unknown) => {
          if (active) setMessage(describeFailure(error));
        });
    // Opening the picker is one of the moments the active device is read again.
    void refreshPlayback().catch(() => undefined);
    load();
    const timer = window.setInterval(load, DEVICE_REFRESH_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [open, allowed]);

  useEffect(() => {
    if (!open) return;
    popoverRef.current?.focus();
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (
        popoverRef.current?.contains(target) ||
        buttonRef.current?.contains(target)
      )
        return;
      setPlacement(null);
    };
    const resize = () => {
      if (slotRef.current) setPlacement(placeBeside(slotRef.current));
    };
    document.addEventListener('pointerdown', outside);
    window.addEventListener('resize', resize);
    return () => {
      document.removeEventListener('pointerdown', outside);
      window.removeEventListener('resize', resize);
    };
  }, [open]);

  function close(): void {
    setPlacement(null);
    setDragging(null);
    buttonRef.current?.focus();
  }

  function toggle(): void {
    if (open) {
      close();
      return;
    }
    setMessage(null);
    if (slotRef.current) setPlacement(placeBeside(slotRef.current));
  }

  function escape(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.stopPropagation();
    close();
  }

  async function run(action: () => Promise<unknown>): Promise<void> {
    setBusy(true);
    setMessage(null);
    try {
      await action();
      close();
    } catch (error) {
      setMessage(describeFailure(error));
    } finally {
      setBusy(false);
    }
  }

  function chooseThisApp(): void {
    // The SDK needs this inside the click itself, before anything is awaited.
    activatePlayback();
    void run(() => playHere(''));
  }

  // Approval happens in the browser and can take minutes, so it doesn't lock the list.
  function allow(): void {
    setMessage(null);
    allowDeviceControl().catch((error: unknown) =>
      setMessage(describeFailure(error)),
    );
  }

  function commitVolume(): void {
    if (dragging === null) return;
    onCommand({ op: 'setVolume', volume: dragging / 100 });
    setDragging(null);
  }

  const others = (devices ?? []).filter((device) => !device.thisApp);
  const hereActive = tier === 'active';
  const remote = isRemote(playback) ? playback?.device : null;
  const otherComputer = others.some((device) => device.kind === 'computer');
  const thisAppNote =
    tier === 'unsupported'
      ? 'Playback in Melora isn’t available on this system.'
      : tier === 'connecting'
        ? 'Connecting to Spotify…'
        : sdkReady
          ? hereActive
            ? 'Computer · Active'
            : 'Computer'
          : 'Turn on Playback in Melora in Settings.';

  return (
    <div className="player-device" ref={slotRef}>
      <span className="eyebrow">Playback device</span>
      <p className="player-device-line">
        <span className="player-device-name">{deviceName(playback)}</span>
        <span aria-hidden="true">&nbsp;·&nbsp;</span>
        <button
          ref={buttonRef}
          className="link-button"
          aria-haspopup="dialog"
          aria-expanded={open}
          onClick={toggle}
          onKeyDown={open ? escape : undefined}
        >
          Choose device
        </button>
      </p>
      {placement && (
        <div
          ref={popoverRef}
          className="device-popover"
          role="dialog"
          aria-label="Choose a Spotify device"
          tabIndex={-1}
          style={placement}
          onKeyDown={escape}
        >
          <p className="eyebrow">Spotify devices</p>
          {!signedIn ? (
            <>
              <p className="device-hint">
                Sign in with Spotify in Settings to choose a device.
              </p>
              <button
                className="link-button device-action"
                onClick={() => {
                  setPlacement(null);
                  onOpenSettings();
                }}
              >
                Open Settings
              </button>
            </>
          ) : !allowed ? (
            <>
              <p className="device-hint">
                Spotify needs your permission before Melora can see and control
                your devices.
              </p>
              <button className="action-button device-action" onClick={allow}>
                Allow device control
              </button>
            </>
          ) : (
            <>
              <ul className="device-list">
                <li>
                  <button
                    className="device-option"
                    aria-current={hereActive ? 'true' : undefined}
                    disabled={!sdkReady || busy}
                    onClick={chooseThisApp}
                  >
                    <span>Melora (this app)</span>
                    <span className="device-detail">{thisAppNote}</span>
                  </button>
                </li>
                {others.map((device) => (
                  <li key={device.key}>
                    <button
                      className="device-option"
                      aria-current={device.active ? 'true' : undefined}
                      disabled={device.restricted || busy}
                      onClick={() =>
                        void run(() => transferPlayback(device.key))
                      }
                    >
                      <span>{device.name || 'Spotify device'}</span>
                      <span className="device-detail">
                        {deviceKindLabel[device.kind]}
                        {device.active ? ' · Active' : ''}
                        {device.restricted
                          ? ' · Spotify doesn’t allow controlling this device from other apps.'
                          : ''}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
              {devices === null && !message && (
                <p className="device-hint" role="status">
                  Looking for devices…
                </p>
              )}
              {devices !== null && !otherComputer && (
                <p className="device-hint">
                  Open Spotify on this PC, then choose it here.
                </p>
              )}
              {playback && isLive(playback) && hereActive && (
                <label className="device-volume">
                  <span>Volume (Melora) {volume}%</span>
                  <input
                    type="range"
                    min="0"
                    max="100"
                    value={volume}
                    onChange={(event) => onVolume(Number(event.target.value))}
                    onPointerUp={(event) =>
                      onVolumeCommit(Number(event.currentTarget.value))
                    }
                    onKeyUp={(event) =>
                      onVolumeCommit(Number(event.currentTarget.value))
                    }
                    onBlur={(event) =>
                      onVolumeCommit(Number(event.currentTarget.value))
                    }
                  />
                </label>
              )}
              {remote &&
                !remote.restricted &&
                (remote.supportsVolume ? (
                  <label className="device-volume">
                    <span>
                      Volume ({remote.name || 'Spotify device'}){' '}
                      {dragging ?? remote.volumePercent ?? 0}%
                    </span>
                    <input
                      type="range"
                      min="0"
                      max="100"
                      value={dragging ?? remote.volumePercent ?? 0}
                      onChange={(event) =>
                        setDragging(Number(event.target.value))
                      }
                      onPointerUp={commitVolume}
                      onKeyUp={commitVolume}
                      onBlur={commitVolume}
                    />
                  </label>
                ) : (
                  <p className="device-hint">
                    This device’s volume can’t be changed from other apps.
                  </p>
                ))}
            </>
          )}
          {message && (
            <p className="error-message device-error" role="alert">
              {message}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
