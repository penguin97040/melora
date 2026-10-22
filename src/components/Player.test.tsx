import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { Player } from './Player';
import { playbackStatus } from '../native/playbackFixture';
import type { PlaybackStatus } from '../native/client';
import { previewTracks } from '../preview';

const track = previewTracks[0]!;

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
    },
    ...extra,
  });
}

function renderPlayer(playback: PlaybackStatus | null) {
  const onCommand = vi.fn();
  const onOpenInSpotify = vi.fn();
  render(
    <Player
      track={track}
      playback={playback}
      onCommand={onCommand}
      onOpenInSpotify={onOpenInSpotify}
    />,
  );
  return { onCommand, onOpenInSpotify };
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
          onCommand={vi.fn()}
          onOpenInSpotify={vi.fn()}
        />,
      );
      expect(screen.getByLabelText('Player preview')).toBeInTheDocument();
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

  it('shows a plain square when there is no allowed artwork', () => {
    renderPlayer(
      live({
        nowPlaying: {
          name: 'Song',
          artists: [],
          album: '',
          spotifyUrl: null,
          imageUrl: null,
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
