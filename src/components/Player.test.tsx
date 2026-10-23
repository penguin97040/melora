import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { Player } from './Player';
import { playbackStatus } from '../native/playbackFixture';
import type { PlaybackStatus } from '../native/client';
import { previewTracks } from '../preview';

const track = previewTracks[0]!;
const slot = <div data-testid="device-slot" />;

function live(extra: Partial<PlaybackStatus> = {}): PlaybackStatus {
  return playbackStatus({
    tier: 'active',
    paused: false,
    positionMs: 20_000,
    durationMs: 180_000,
    updatedAtMs: Date.now(),
    nowPlaying: {
      name: 'Song',
      artists: ['Artist', 'Guest'],
      album: 'Album',
      spotifyUrl: 'https://open.spotify.com/track/x',
      imageUrl: 'https://i.scdn.co/image/album',
      liked: null,
    },
    ...extra,
  });
}

function renderPlayer(
  playback: PlaybackStatus | null,
  likeMessage: string | null = null,
) {
  const onCommand = vi.fn();
  const onToggleLike = vi.fn();
  const onOpenInSpotify = vi.fn();
  render(
    <Player
      track={track}
      playback={playback}
      devicePicker={slot}
      likeMessage={likeMessage}
      onCommand={onCommand}
      onToggleLike={onToggleLike}
      onOpenInSpotify={onOpenInSpotify}
    />,
  );
  return { onCommand, onToggleLike, onOpenInSpotify };
}

describe('player panel', () => {
  it('stays the labelled preview unless Melora is the active device', () => {
    for (const playback of [
      null,
      playbackStatus(),
      playbackStatus({ tier: 'ready' }),
    ]) {
      const { unmount } = render(
        <Player
          track={track}
          playback={playback}
          devicePicker={slot}
          likeMessage={null}
          onCommand={vi.fn()}
          onToggleLike={vi.fn()}
          onOpenInSpotify={vi.fn()}
        />,
      );
      expect(screen.getByLabelText('Player preview')).toBeInTheDocument();
      // Synthetic content carries no Spotify brand and no + control.
      expect(
        screen.queryByRole('img', { name: 'Spotify' }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole('button', { name: /Liked Songs/ }),
      ).not.toBeInTheDocument();
      expect(screen.getByTestId('device-slot')).toBeInTheDocument();
      expect(screen.getByText(track.title)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Play' })).toBeDisabled();
      unmount();
    }
  });

  it('shows the real item with unaltered artwork and attribution', async () => {
    const { onOpenInSpotify } = renderPlayer(live());
    expect(screen.getByLabelText('Player')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Song' })).toBeInTheDocument();
    expect(screen.getByText('Artist, Guest')).toBeInTheDocument();
    expect(screen.getByText('Playing in Melora')).toBeInTheDocument();
    const artwork = screen.getByRole('img', { name: 'Artwork for Album' });
    expect(artwork).toHaveAttribute('src', 'https://i.scdn.co/image/album');
    expect(artwork).toHaveClass('cover-art');
    // Nothing is drawn over the artwork.
    expect(artwork.parentElement?.children).toHaveLength(1);
    expect(screen.queryByText(/synthetic/i)).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole('button', { name: 'Open in Spotify' }),
    );
    expect(onOpenInSpotify).toHaveBeenCalledWith(
      'https://open.spotify.com/track/x',
    );
  });

  it('attributes the item with the official Spotify logo, not over the artwork', () => {
    renderPlayer(live());
    const logo = screen.getByRole('img', { name: 'Spotify' });
    expect(logo).toHaveClass('spotify-logo');
    expect(logo.getAttribute('src')).toMatch(/Full_Logo_White_RGB/);
    expect(logo.closest('.panel-label')).not.toBeNull();
    expect(logo.closest('.player-artwork')).toBeNull();
  });

  it('puts the + at the end of the title line and shows its state', async () => {
    const { onToggleLike } = renderPlayer(live());
    const add = screen.getByRole('button', { name: 'Add to Liked Songs' });
    expect(add.closest('.title-row')).toContainElement(
      screen.getByRole('heading', { name: 'Song' }),
    );
    expect(add.querySelector('img')?.getAttribute('src')).toMatch(
      /like-icon-like\b/,
    );
    await userEvent.click(add);
    expect(onToggleLike).toHaveBeenCalledTimes(1);
  });

  it('shows a liked track as liked, and Spotify’s message in the mode line', () => {
    const playback = live();
    renderPlayer(
      { ...playback, nowPlaying: { ...playback.nowPlaying!, liked: true } },
      'Added to Liked Songs',
    );
    const remove = screen.getByRole('button', {
      name: 'Remove from Liked Songs',
    });
    expect(remove.querySelector('img')?.getAttribute('src')).toMatch(
      /like-icon-liked/,
    );
    expect(screen.getByRole('status')).toHaveTextContent(
      'Added to Liked Songs',
    );
  });

  it('has no + for an episode', () => {
    const playback = live();
    renderPlayer({
      ...playback,
      nowPlaying: {
        ...playback.nowPlaying!,
        spotifyUrl: 'https://open.spotify.com/episode/x',
      },
    });
    expect(
      screen.queryByRole('button', { name: /Liked Songs/ }),
    ).not.toBeInTheDocument();
  });

  function episode(extra: Partial<PlaybackStatus> = {}): PlaybackStatus {
    const playback = live({ paused: true, ...extra });
    return {
      ...playback,
      nowPlaying: {
        ...playback.nowPlaying!,
        spotifyUrl: 'https://open.spotify.com/episode/x',
      },
    };
  }

  it('skips 15 seconds in place of Previous and Next for an episode', async () => {
    const { onCommand } = renderPlayer(episode());
    expect(
      screen.queryByRole('button', { name: 'Previous track' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Next track' }),
    ).not.toBeInTheDocument();
    const buttons = screen
      .getByRole('group', { name: 'Player controls' })
      .querySelectorAll('button');
    expect(
      [...buttons].map((button) => button.getAttribute('aria-label')),
    ).toEqual([
      'Shuffle: Off',
      'Back 15 seconds',
      'Play',
      'Forward 15 seconds',
      'Repeat: Off',
    ]);
    await userEvent.click(
      screen.getByRole('button', { name: 'Back 15 seconds' }),
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Forward 15 seconds' }),
    );
    expect(onCommand.mock.calls.map(([command]) => command)).toEqual([
      { op: 'seek', positionMs: 5_000 },
      { op: 'seek', positionMs: 35_000 },
    ]);
  });

  it('keeps an episode skip within the episode', async () => {
    const { onCommand } = renderPlayer(episode({ positionMs: 4_000 }));
    await userEvent.click(
      screen.getByRole('button', { name: 'Back 15 seconds' }),
    );
    expect(onCommand).toHaveBeenLastCalledWith({ op: 'seek', positionMs: 0 });
  });

  it('keeps a forward skip at the end of the episode', async () => {
    const { onCommand } = renderPlayer(episode({ positionMs: 175_000 }));
    await userEvent.click(
      screen.getByRole('button', { name: 'Forward 15 seconds' }),
    );
    expect(onCommand).toHaveBeenLastCalledWith({
      op: 'seek',
      positionMs: 180_000,
    });
  });

  it('disables the episode skips when Spotify disallows seeking', () => {
    renderPlayer(
      episode({
        disallows: {
          pause: false,
          resume: false,
          next: false,
          previous: false,
          seek: true,
        },
      }),
    );
    expect(
      screen.getByRole('button', { name: 'Back 15 seconds' }),
    ).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Forward 15 seconds' }),
    ).toBeDisabled();
  });

  it('sends each control to Rust and reflects the real modes', async () => {
    const { onCommand } = renderPlayer(
      live({ shuffle: true, repeat: 'context' }),
    );
    expect(screen.getByRole('status')).toHaveTextContent(
      'Shuffle: On · Repeat: All',
    );
    await userEvent.click(screen.getByRole('button', { name: 'Pause' }));
    await userEvent.click(screen.getByRole('button', { name: 'Next track' }));
    await userEvent.click(
      screen.getByRole('button', { name: 'Previous track' }),
    );
    await userEvent.click(screen.getByRole('button', { name: 'Shuffle: On' }));
    await userEvent.click(screen.getByRole('button', { name: 'Repeat: All' }));
    expect(onCommand.mock.calls.map(([command]) => command)).toEqual([
      { op: 'togglePlay' },
      { op: 'next' },
      { op: 'previous' },
      { op: 'setShuffle', state: false },
      { op: 'setRepeat', mode: 'track' },
    ]);
  });

  it('disables what Spotify disallows', () => {
    renderPlayer(
      live({
        paused: true,
        disallows: {
          pause: false,
          resume: true,
          next: true,
          previous: false,
          seek: true,
        },
      }),
    );
    expect(screen.getByRole('button', { name: 'Play' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Next track' })).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Previous track' }),
    ).toBeEnabled();
    expect(screen.getByLabelText('Track position')).toBeDisabled();
  });

  it('seeks once, on release, not while dragging', () => {
    const { onCommand } = renderPlayer(live({ paused: true }));
    const slider = screen.getByLabelText('Track position');
    fireEvent.change(slider, { target: { value: '60000' } });
    fireEvent.change(slider, { target: { value: '90000' } });
    expect(onCommand).not.toHaveBeenCalled();
    expect(screen.getByText('1:30')).toBeInTheDocument();
    fireEvent.pointerUp(slider);
    expect(onCommand).toHaveBeenCalledTimes(1);
    expect(onCommand).toHaveBeenCalledWith({ op: 'seek', positionMs: 90_000 });
  });

  it('shows and controls another active device', async () => {
    const { onCommand } = renderPlayer(
      live({
        tier: 'ready',
        device: {
          key: '0123456789abcdef',
          name: 'Kitchen',
          kind: 'speaker',
          active: true,
          restricted: false,
          thisApp: false,
          supportsVolume: true,
          volumePercent: 40,
        },
      }),
    );
    expect(screen.getByLabelText('Player')).toBeInTheDocument();
    expect(screen.getByText('Playing on Kitchen')).toBeInTheDocument();
    expect(screen.getByText(/Spotify plays on another device/)).toBeVisible();
    expect(screen.getByTestId('device-slot')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Pause' }));
    expect(onCommand).toHaveBeenCalledWith({ op: 'togglePlay' });
  });

  it('disables every control for a restricted device', () => {
    renderPlayer(
      live({
        tier: 'off',
        device: {
          key: '0123456789abcdef',
          name: 'Car',
          kind: 'automobile',
          active: true,
          restricted: true,
          thisApp: false,
          supportsVolume: false,
          volumePercent: null,
        },
      }),
    );
    const controls = screen.getByRole('group', { name: 'Player controls' });
    for (const button of controls.querySelectorAll('button'))
      expect(button).toBeDisabled();
    expect(screen.getByLabelText('Track position')).toBeDisabled();
    expect(
      screen.getByText(
        'Spotify doesn’t allow controlling this device from other apps.',
      ),
    ).toBeInTheDocument();
  });

  it('shows a plain square when there is no allowed artwork', () => {
    renderPlayer(
      live({
        nowPlaying: {
          name: 'Song',
          artists: [],
          album: '',
          spotifyUrl: null,
          imageUrl: null,
          liked: null,
        },
      }),
    );
    expect(
      screen.getByRole('img', { name: 'No artwork' }),
    ).toBeEmptyDOMElement();
    expect(
      screen.queryByRole('button', { name: 'Open in Spotify' }),
    ).not.toBeInTheDocument();
  });
});
