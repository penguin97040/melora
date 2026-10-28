import { useState } from 'react';
import type { ReactNode } from 'react';
import type { PlaybackStatus, PlayerCommand } from '../native/client';
import type { RepeatMode } from '../native/bindings/RepeatMode';
import {
  isEpisode,
  isLive,
  isRemote,
  likeable,
  SKIP_MS,
  skipTarget,
} from '../features/playback';
import { usePlaybackPosition } from '../features/usePlaybackPosition';
import { trackTime } from '../features/library';
// Spotify's own assets, shipped as the original files (never inlined, which rewrites the SVG
// text). Design Guidelines; see THIRD_PARTY_NOTICES.md.
import spotifyLogo from '../assets/brand/Full_Logo_White_RGB.svg?no-inline';
import likeIcon from '../assets/brand/like-icon-like.svg?no-inline';
import likedIcon from '../assets/brand/like-icon-liked.svg?no-inline';
import {
  NextIcon,
  PauseIcon,
  PlayIcon,
  PreviousIcon,
  RepeatIcon,
  ShuffleIcon,
  SkipBackIcon,
  SkipForwardIcon,
} from './playerIcons';

const repeatLabel: Record<RepeatMode, 'Off' | 'All' | 'Song'> = {
  off: 'Off',
  context: 'All',
  track: 'Song',
};
const nextRepeat: Record<RepeatMode, RepeatMode> = {
  off: 'context',
  context: 'track',
  track: 'off',
};

const restrictedAll = {
  pause: true,
  resume: true,
  next: true,
  previous: true,
  seek: true,
};

type LiveProps = {
  playback: PlaybackStatus;
  /** The Playback device line, in the slot the approved layout reserves for it. */
  devicePicker: ReactNode;
  /** Spotify's "Added to Liked Songs" or "Removed from Liked Songs", shown for a moment. */
  likeMessage: string | null;
  onCommand: (command: PlayerCommand) => void;
  onToggleLike: () => void;
  onOpenInSpotify: (url: string) => void;
};

export function Player({
  playback,
  ...live
}: Omit<LiveProps, 'playback'> & {
  playback: PlaybackStatus | null;
}) {
  // While a Spotify device is active (Melora itself or another one), the panel shows the
  // real item and controls. Otherwise it waits, with Melora's own artwork. One shell for both
  // keeps the device picker in the same place, so an open list survives the switch (opening
  // it reads the devices, which can be what makes something live).
  const active = playback !== null && isLive(playback) ? playback : null;
  const { devicePicker, ...controls } = live;
  return (
    <aside
      className="player panel"
      aria-label={active ? 'Player' : 'Player: nothing playing'}
    >
      {active ? <LiveHeader playback={active} /> : <IdleHeader />}
      <div className="player-body">
        {active ? (
          <LiveControls playback={active} {...controls} />
        ) : (
          <IdleControls message={live.likeMessage} />
        )}
        {devicePicker}
        <p className="player-footnote">
          {active
            ? liveFootnote(active)
            : 'Melora shows and controls what plays on your Spotify devices. Choose device to pick one.'}
        </p>
      </div>
    </aside>
  );
}

function liveFootnote(playback: PlaybackStatus): string {
  const remote = isRemote(playback);
  if (remote && playback.device?.restricted === true)
    return 'Spotify doesn’t allow controlling this device from other apps.';
  return remote
    ? 'Spotify plays on another device. Track details and artwork come from Spotify.'
    : 'Playing through Spotify in Melora. Track details and artwork come from Spotify.';
}

function LiveHeader({ playback }: { playback: PlaybackStatus }) {
  const { nowPlaying } = playback;
  return (
    <>
      <div className="panel-label brand-label">
        <span>Now playing</span>
        <img className="spotify-logo" src={spotifyLogo} alt="Spotify" />
      </div>
      <div className="player-artwork">
        {nowPlaying?.imageUrl ? (
          <img
            className="cover-art"
            src={nowPlaying.imageUrl}
            alt={`Artwork for ${nowPlaying.album || nowPlaying.name}`}
            loading="lazy"
            referrerPolicy="no-referrer"
          />
        ) : (
          <div
            className="cover-art cover-missing"
            role="img"
            aria-label="No artwork"
          />
        )}
      </div>
    </>
  );
}

function LiveControls({
  playback,
  likeMessage,
  onCommand,
  onToggleLike,
  onOpenInSpotify,
}: Omit<LiveProps, 'devicePicker'>) {
  const position = usePlaybackPosition(playback);
  // The slider's value while it's being dragged; the seek is sent on release only.
  const [dragging, setDragging] = useState<number | null>(null);
  const { nowPlaying, paused } = playback;
  const remote = isRemote(playback);
  // Spotify accepts no commands for a restricted device, so every control is disabled.
  const restricted = remote && playback.device?.restricted === true;
  const disallows = restricted ? restrictedAll : playback.disallows;
  const place = remote
    ? `on ${playback.device?.name || 'another device'}`
    : 'in Melora';
  const repeat = repeatLabel[playback.repeat];
  const shown = dragging ?? position;
  const playDisabled = paused ? disallows.resume : disallows.pause;
  const spotifyUrl = nowPlaying?.spotifyUrl;
  const liked = nowPlaying?.liked === true;
  // Podcasts skip 15 seconds where tracks have Previous and Next, through the same seek.
  const episode = isEpisode(spotifyUrl);
  const seekDisabled = disallows.seek || playback.durationMs === 0;

  function skip(deltaMs: number): void {
    onCommand({
      op: 'seek',
      positionMs: skipTarget(position, playback.durationMs, deltaMs),
    });
  }

  function commitSeek(): void {
    if (dragging === null) return;
    onCommand({ op: 'seek', positionMs: dragging });
    setDragging(null);
  }

  return (
    <>
      <p className="eyebrow">
        {paused ? 'Paused' : 'Playing'} {place}
      </p>
      <div className="title-row">
        <h2 className="track-title">{nowPlaying?.name ?? 'Spotify'}</h2>
        {likeable(spotifyUrl) && (
          <button
            className="like-button"
            aria-label={
              liked ? 'Remove from Liked Songs' : 'Add to Liked Songs'
            }
            onClick={onToggleLike}
          >
            <img src={liked ? likedIcon : likeIcon} alt="" />
          </button>
        )}
      </div>
      <p className="artist-name">{nowPlaying?.artists.join(', ')}</p>
      <p className="muted">{nowPlaying?.album}</p>
      <div className="playback-notice">
        <span className="status-dot" />
        From Spotify
        {spotifyUrl && (
          <>
            {' · '}
            <button
              className="link-button"
              onClick={() => onOpenInSpotify(spotifyUrl)}
            >
              Open in Spotify
            </button>
          </>
        )}
      </div>
      <label className="sr-only" htmlFor="seek">
        Track position
      </label>
      <input
        id="seek"
        type="range"
        min="0"
        max={playback.durationMs}
        step="1000"
        value={Math.min(shown, playback.durationMs)}
        disabled={seekDisabled}
        onChange={(event) => setDragging(Number(event.target.value))}
        onPointerUp={commitSeek}
        onKeyUp={commitSeek}
        onBlur={commitSeek}
      />
      <div className="time-labels">
        <span>{trackTime(shown)}</span>
        <span>{trackTime(playback.durationMs)}</span>
      </div>
      <div className="transport" role="group" aria-label="Player controls">
        <button
          className={playback.shuffle ? 'mode-active' : ''}
          aria-label={`Shuffle: ${playback.shuffle ? 'On' : 'Off'}`}
          aria-pressed={playback.shuffle}
          disabled={restricted}
          onClick={() =>
            onCommand({ op: 'setShuffle', state: !playback.shuffle })
          }
        >
          <ShuffleIcon />
        </button>
        {episode ? (
          <button
            aria-label="Back 15 seconds"
            disabled={seekDisabled}
            onClick={() => skip(-SKIP_MS)}
          >
            <SkipBackIcon />
          </button>
        ) : (
          <button
            aria-label="Previous track"
            disabled={disallows.previous}
            onClick={() => onCommand({ op: 'previous' })}
          >
            <PreviousIcon />
          </button>
        )}
        <button
          className="play-button"
          aria-label={paused ? 'Play' : 'Pause'}
          disabled={playDisabled}
          onClick={() => onCommand({ op: 'togglePlay' })}
        >
          {paused ? <PlayIcon /> : <PauseIcon />}
        </button>
        {episode ? (
          <button
            aria-label="Forward 15 seconds"
            disabled={seekDisabled}
            onClick={() => skip(SKIP_MS)}
          >
            <SkipForwardIcon />
          </button>
        ) : (
          <button
            aria-label="Next track"
            disabled={disallows.next}
            onClick={() => onCommand({ op: 'next' })}
          >
            <NextIcon />
          </button>
        )}
        <button
          className={repeat !== 'Off' ? 'mode-active' : ''}
          aria-label={`Repeat: ${repeat}`}
          aria-pressed={repeat !== 'Off'}
          disabled={restricted}
          onClick={() =>
            onCommand({ op: 'setRepeat', mode: nextRepeat[playback.repeat] })
          }
        >
          <RepeatIcon one={repeat === 'Song'} />
        </button>
      </div>
      <p className="mode-status" role="status">
        {likeMessage ??
          `Shuffle: ${playback.shuffle ? 'On' : 'Off'} · Repeat: ${repeat}`}
      </p>
    </>
  );
}

/** Nothing plays on any device: the same rows as the live player, so the layout holds. */
function IdleHeader() {
  return (
    <>
      <div className="panel-label">
        <span>Now playing</span>
        <span>Idle</span>
      </div>
      <div className="player-artwork">
        <div className="cover" role="img" aria-label="Melora artwork">
          <div className="cover-orbit" />
          <span className="cover-mark">m.</span>
          <span className="cover-caption">MELORA / NOTHING PLAYING</span>
        </div>
      </div>
    </>
  );
}

function IdleControls({
  message,
}: {
  /** A short confirmation, like "Added to Mine", in place of the mode line. */
  message: string | null;
}) {
  return (
    <>
      <p className="eyebrow">Ready when you are</p>
      <h2 className="track-title">Nothing playing</h2>
      <p className="artist-name">Start Spotify on any device</p>
      <p className="muted">and it shows here.</p>
      <div className="playback-notice">
        <span className="status-dot" />
        Waiting for Spotify
      </div>
      <label className="sr-only" htmlFor="seek">
        Track position
      </label>
      <input id="seek" type="range" min="0" max="100" value="0" disabled />
      <div className="time-labels">
        <span>0:00</span>
        <span>0:00</span>
      </div>
      <div className="transport" role="group" aria-label="Player controls">
        <button disabled aria-label="Shuffle">
          <ShuffleIcon />
        </button>
        <button disabled aria-label="Previous track">
          <PreviousIcon />
        </button>
        <button disabled className="play-button" aria-label="Play">
          <PlayIcon />
        </button>
        <button disabled aria-label="Next track">
          <NextIcon />
        </button>
        <button disabled aria-label="Repeat">
          <RepeatIcon one={false} />
        </button>
      </div>
      <p className="mode-status" role="status">
        {message ?? 'The controls work once something plays.'}
      </p>
    </>
  );
}
