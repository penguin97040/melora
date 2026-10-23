import { useState } from 'react';
import type { ReactNode } from 'react';
import type { PreviewTrack } from '../preview';
import type { PlaybackStatus, PlayerCommand } from '../native/client';
import type { RepeatMode } from '../native/bindings/RepeatMode';
import { formatTime, isLive, isRemote, likeable } from '../features/playback';
import { usePlaybackPosition } from '../features/usePlaybackPosition';
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
  track,
  playback,
  ...live
}: Omit<LiveProps, 'playback'> & {
  track: PreviewTrack;
  playback: PlaybackStatus | null;
}) {
  // While a Spotify device is active (Melora itself or another one), the panel shows the
  // real item and controls. Otherwise it stays the labelled synthetic preview.
  if (playback && isLive(playback)) {
    return <LivePlayer playback={playback} {...live} />;
  }
  return <PreviewPlayer track={track} devicePicker={live.devicePicker} />;
}

function LivePlayer({
  playback,
  devicePicker,
  likeMessage,
  onCommand,
  onToggleLike,
  onOpenInSpotify,
}: LiveProps) {
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

  function commitSeek(): void {
    if (dragging === null) return;
    onCommand({ op: 'seek', positionMs: dragging });
    setDragging(null);
  }

  return (
    <aside className="player panel" aria-label="Player">
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
      <div className="player-body">
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
          disabled={disallows.seek || playback.durationMs === 0}
          onChange={(event) => setDragging(Number(event.target.value))}
          onPointerUp={commitSeek}
          onKeyUp={commitSeek}
          onBlur={commitSeek}
        />
        <div className="time-labels">
          <span>{formatTime(shown)}</span>
          <span>{formatTime(playback.durationMs)}</span>
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
          <button
            aria-label="Previous track"
            disabled={disallows.previous}
            onClick={() => onCommand({ op: 'previous' })}
          >
            <PreviousIcon />
          </button>
          <button
            className="play-button"
            aria-label={paused ? 'Play' : 'Pause'}
            disabled={playDisabled}
            onClick={() => onCommand({ op: 'togglePlay' })}
          >
            {paused ? <PlayIcon /> : <PauseIcon />}
          </button>
          <button
            aria-label="Next track"
            disabled={disallows.next}
            onClick={() => onCommand({ op: 'next' })}
          >
            <NextIcon />
          </button>
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
        {devicePicker}
        <p className="player-footnote">
          {restricted
            ? 'Spotify doesn’t allow controlling this device from other apps.'
            : remote
              ? 'Spotify plays on another device. Track details and artwork come from Spotify.'
              : 'Playing through Spotify in Melora. Track details and artwork come from Spotify.'}
        </p>
      </div>
    </aside>
  );
}

function PreviewPlayer({
  track,
  devicePicker,
}: {
  track: PreviewTrack;
  devicePicker: ReactNode;
}) {
  const [shuffle, setShuffle] = useState(false);
  const [repeat, setRepeat] = useState<'Off' | 'All' | 'Song'>('Off');

  return (
    <aside className="player panel" aria-label="Player preview">
      <div className="panel-label">
        <span>Now playing</span>
        <span>Preview</span>
      </div>
      <div className="player-artwork">
        <div
          className="cover"
          role="img"
          aria-label="Abstract synthetic preview artwork"
        >
          <div className="cover-orbit" />
          <span className="cover-mark">m.</span>
          <span className="cover-caption">AFTER THE QUIET / 001</span>
        </div>
      </div>
      <div className="player-body">
        <p className="eyebrow">Selected preview track</p>
        <h2 className="track-title">{track.title}</h2>
        <p className="artist-name">{track.artist}</p>
        <p className="muted">{track.album}</p>
        <div className="playback-notice">
          <span className="status-dot" />
          Playback unavailable in preview
        </div>
        <label className="sr-only" htmlFor="seek">
          Track position
        </label>
        <input id="seek" type="range" min="0" max="100" value="0" disabled />
        <div className="time-labels">
          <span>0:00</span>
          <span>{track.duration}</span>
        </div>
        <div
          className="transport"
          role="group"
          aria-label="Preview player controls"
        >
          <button
            className={shuffle ? 'mode-active' : ''}
            aria-label={`Shuffle: ${shuffle ? 'On' : 'Off'} (preview)`}
            aria-pressed={shuffle}
            aria-describedby="preview-modes-note"
            onClick={() => setShuffle((current) => !current)}
          >
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
          <button
            className={repeat !== 'Off' ? 'mode-active' : ''}
            aria-label={`Repeat: ${repeat} (preview)`}
            aria-pressed={repeat !== 'Off'}
            aria-describedby="preview-modes-note"
            onClick={() =>
              setRepeat((current) =>
                current === 'Off' ? 'All' : current === 'All' ? 'Song' : 'Off',
              )
            }
          >
            <RepeatIcon one={repeat === 'Song'} />
          </button>
        </div>
        <p className="mode-status" role="status">
          Shuffle: {shuffle ? 'On' : 'Off'} · Repeat: {repeat}
        </p>
        {devicePicker}
        <p className="player-footnote" id="preview-modes-note">
          Shuffle and repeat are preview selections only; track order and audio
          are unchanged. Modes reset on relaunch. Real playback shows here once
          enabled in Settings. These tracks and artwork are synthetic.
        </p>
      </div>
    </aside>
  );
}
