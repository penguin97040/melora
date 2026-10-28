import { useEffect, useRef, useState } from 'react';

type Props = {
  /** A song waiting to go into the new playlist, named in the hint. */
  adding: string | null;
  busy: boolean;
  onCreate: (name: string) => void;
  onCancel: () => void;
};

/** Spotify's name limit is undocumented; Melora keeps names to this (as `ora-core` does). */
const MAX_NAME = 100;

/**
 * The sidebar's form for a new private playlist. Escape or Cancel closes it, but not while
 * it's creating: the playlist is on its way and will open.
 */
export function NewPlaylist({ adding, busy, onCreate, onCancel }: Props) {
  const [name, setName] = useState('');
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const wanted = name.trim();

  return (
    <form
      className="new-playlist"
      aria-label="New playlist"
      onSubmit={(event) => {
        event.preventDefault();
        if (wanted && !busy) onCreate(wanted);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault();
          if (!busy) onCancel();
        }
      }}
    >
      <label htmlFor="new-playlist-name">Playlist name</label>
      <input
        ref={input}
        id="new-playlist-name"
        value={name}
        maxLength={MAX_NAME}
        autoComplete="off"
        onChange={(event) => setName(event.target.value)}
      />
      <span className="playlist-subtitle">
        {adding
          ? `Private, on your Spotify account. “${adding}” goes in first.`
          : 'Private, on your Spotify account.'}
      </span>
      <span className="new-playlist-actions">
        <button
          type="submit"
          className="link-button"
          disabled={!wanted || busy}
        >
          {busy ? 'Creating…' : 'Create'}
        </button>
        <button
          type="button"
          className="link-button"
          disabled={busy}
          onClick={onCancel}
        >
          Cancel
        </button>
      </span>
    </form>
  );
}
