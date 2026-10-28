import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEVICE_REFRESH_MS, DevicePicker } from './DevicePicker';
import {
  activatePlayback,
  allowDeviceControl,
  listDevices,
  playHere,
  refreshPlayback,
  transferPlayback,
} from '../native/client';
import { playbackStatus } from '../native/playbackFixture';
import type { Device, PlaybackStatus, SpotifyStatus } from '../native/client';
import type { Feature } from '../native/bindings/Feature';

vi.mock('../native/client', () => ({
  activatePlayback: vi.fn(),
  allowDeviceControl: vi.fn(),
  listDevices: vi.fn(),
  playHere: vi.fn(),
  refreshPlayback: vi.fn(),
  transferPlayback: vi.fn(),
}));

function spotify(granted: Feature[]): SpotifyStatus {
  return {
    auth: {
      kind: 'signedIn',
      profile: null,
      sessionOnly: false,
      granted,
      consenting: false,
    },
    clientIdFromEnvironment: false,
    clientIdHint: null,
    lastError: null,
    redirectUri: 'http://127.0.0.1:43821/callback',
  };
}

const signedOut: SpotifyStatus = {
  ...spotify([]),
  auth: { kind: 'signedOut' },
};
const control: Feature[] = [
  'SignIn',
  'ConnectControl',
  'DevicePicker',
  'TransferPlayback',
];

function device(extra: Partial<Device>): Device {
  return {
    key: '0123456789abcdef',
    name: 'Phone',
    kind: 'smartphone',
    active: false,
    restricted: false,
    thisApp: false,
    supportsVolume: false,
    volumePercent: null,
    ...extra,
  };
}

const devices = [
  device({ key: 'aaaa', name: 'Melora', kind: 'computer', thisApp: true }),
  device({ key: 'bbbb', name: 'Phone', active: true }),
  device({ key: 'cccc', name: 'Car', kind: 'automobile', restricted: true }),
];

function renderPicker(
  status: SpotifyStatus | null,
  playback: PlaybackStatus | null = playbackStatus({ tier: 'ready' }),
) {
  const handlers = {
    onVolume: vi.fn(),
    onVolumeCommit: vi.fn(),
    onCommand: vi.fn(),
    onOpenSettings: vi.fn(),
  };
  const view = render(
    <DevicePicker
      spotify={status}
      playback={playback}
      volume={70}
      {...handlers}
    />,
  );
  return { ...handlers, ...view };
}

const chooseButton = () =>
  screen.getByRole('button', { name: 'Choose device' });

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listDevices).mockResolvedValue(devices);
  vi.mocked(refreshPlayback).mockResolvedValue(undefined);
  vi.mocked(transferPlayback).mockResolvedValue(undefined);
  vi.mocked(playHere).mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe('playback device', () => {
  it('names the active device on its line', () => {
    const { rerender } = renderPicker(null, null);
    expect(screen.getByText('No device connected')).toBeInTheDocument();
    const props = {
      spotify: null,
      volume: 70,
      onVolume: vi.fn(),
      onVolumeCommit: vi.fn(),
      onCommand: vi.fn(),
      onOpenSettings: vi.fn(),
    };
    rerender(
      <DevicePicker {...props} playback={playbackStatus({ tier: 'active' })} />,
    );
    expect(screen.getByText('Melora (this app)')).toBeInTheDocument();
    rerender(
      <DevicePicker
        {...props}
        playback={playbackStatus({
          tier: 'ready',
          device: device({ name: 'Kitchen', active: true }),
        })}
      />,
    );
    expect(screen.getByText('Kitchen')).toBeInTheDocument();
  });

  it('points to Settings when signed out, and Escape closes the list', async () => {
    const { onOpenSettings } = renderPicker(signedOut);
    await userEvent.click(chooseButton());
    expect(chooseButton()).toHaveAttribute('aria-expanded', 'true');
    const dialog = screen.getByRole('dialog', {
      name: 'Choose a Spotify device',
    });
    expect(dialog).toHaveTextContent(
      'Sign in with Spotify in Settings to choose a device.',
    );
    expect(dialog).toHaveFocus();
    expect(listDevices).not.toHaveBeenCalled();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(chooseButton()).toHaveFocus();

    await userEvent.click(chooseButton());
    await userEvent.click(
      screen.getByRole('button', { name: 'Open Settings' }),
    );
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('asks for Spotify’s permission first', async () => {
    vi.mocked(allowDeviceControl).mockResolvedValue(spotify(control));
    renderPicker(spotify(['SignIn', 'Profile']));
    await userEvent.click(chooseButton());
    await userEvent.click(
      screen.getByRole('button', { name: 'Allow device control' }),
    );
    expect(allowDeviceControl).toHaveBeenCalledTimes(1);
    expect(listDevices).not.toHaveBeenCalled();
  });

  it('lists the devices and moves playback to the one chosen', async () => {
    renderPicker(spotify(control));
    await userEvent.click(chooseButton());
    expect(refreshPlayback).toHaveBeenCalledTimes(1);
    const phone = await screen.findByRole('button', { name: /^Phone/ });
    expect(phone).toHaveAttribute('aria-current', 'true');
    expect(phone).toHaveTextContent('Phone · Active');
    const car = screen.getByRole('button', { name: /^Car/ });
    expect(car).toBeDisabled();
    expect(car).toHaveTextContent(
      'Spotify doesn’t allow controlling this device from other apps.',
    );
    // Melora is listed once, as this app, first.
    const options = screen.getAllByRole('button', { name: /Melora|Phone|Car/ });
    expect(options[0]).toHaveTextContent('Melora (this app)');
    expect(screen.queryByText(/^Melora$/)).not.toBeInTheDocument();
    expect(
      screen.getByText('Open Spotify on this PC, then choose it here.'),
    ).toBeInTheDocument();

    await userEvent.click(phone);
    expect(transferPlayback).toHaveBeenCalledWith('bbbb');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('plays here from inside the click when Melora is chosen', async () => {
    renderPicker(spotify(control));
    await userEvent.click(chooseButton());
    await userEvent.click(
      await screen.findByRole('button', { name: /^Melora \(this app\)/ }),
    );
    expect(activatePlayback).toHaveBeenCalledTimes(1);
    expect(playHere).toHaveBeenCalledWith('');
  });

  it('can’t choose Melora until its playback is on', async () => {
    renderPicker(spotify(control), playbackStatus({ tier: 'off' }));
    await userEvent.click(chooseButton());
    const here = await screen.findByRole('button', {
      name: /^Melora \(this app\)/,
    });
    expect(here).toBeDisabled();
    expect(here).toHaveTextContent('Turn on Playback in Melora in Settings.');
  });

  it('shows why a choice failed and keeps the list open', async () => {
    vi.mocked(transferPlayback).mockRejectedValue({
      kind: 'deviceUnavailable',
    });
    renderPicker(spotify(control));
    await userEvent.click(chooseButton());
    await userEvent.click(
      await screen.findByRole('button', { name: /^Phone/ }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That device is no longer available. Choose another one.',
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('reads the list again while it is open, and stops once closed', async () => {
    vi.useFakeTimers();
    renderPicker(spotify(control));
    fireEvent.click(chooseButton());
    await act(async () => {});
    expect(listDevices).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(DEVICE_REFRESH_MS);
    });
    expect(listDevices).toHaveBeenCalledTimes(2);
    fireEvent.click(chooseButton());
    await act(async () => {
      vi.advanceTimersByTime(DEVICE_REFRESH_MS * 3);
    });
    expect(listDevices).toHaveBeenCalledTimes(2);
  });

  it('sends another device’s volume once, on release', async () => {
    const { onCommand } = renderPicker(
      spotify(control),
      playbackStatus({
        tier: 'ready',
        device: device({
          name: 'Kitchen',
          kind: 'speaker',
          active: true,
          supportsVolume: true,
          volumePercent: 40,
        }),
      }),
    );
    await userEvent.click(chooseButton());
    const slider = screen.getByRole('slider', { name: /Volume \(Kitchen\)/ });
    fireEvent.change(slider, { target: { value: '55' } });
    fireEvent.change(slider, { target: { value: '65' } });
    expect(onCommand).not.toHaveBeenCalled();
    fireEvent.pointerUp(slider);
    expect(onCommand).toHaveBeenCalledTimes(1);
    expect(onCommand).toHaveBeenCalledWith({ op: 'setVolume', volume: 0.65 });
  });

  it('uses Melora’s own volume setting while Melora plays', async () => {
    const { onVolume, onVolumeCommit } = renderPicker(
      spotify(control),
      playbackStatus({ tier: 'active' }),
    );
    await userEvent.click(chooseButton());
    const slider = screen.getByRole('slider', { name: /Volume \(Melora\)/ });
    expect(slider).toHaveValue('70');
    fireEvent.change(slider, { target: { value: '30' } });
    expect(onVolume).toHaveBeenCalledWith(30);
    // Saved and sent once, on release, not per step of the drag.
    expect(onVolumeCommit).not.toHaveBeenCalled();
    fireEvent.pointerUp(slider);
    expect(onVolumeCommit).toHaveBeenCalledTimes(1);
  });
});
