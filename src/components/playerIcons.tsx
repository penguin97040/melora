// Transport icons shared by the preview and the live player.

export function ShuffleIcon() {
  return (
    <svg className="mode-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M3 6h3c5 0 7 12 12 12h3m-4-4 4 4-4 4M3 18h3c2 0 3-2 4-4m4-4c1-2 2-4 4-4h3m-4-4 4 4-4 4" />
    </svg>
  );
}

export function PreviousIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M5 5h3v14H5zM19 5v14L8 12z" />
    </svg>
  );
}

export function PlayIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M7 4v16l13-8z" />
    </svg>
  );
}

export function PauseIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 5h4v14H6zM14 5h4v14h-4z" />
    </svg>
  );
}

export function NextIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M16 5h3v14h-3zM5 5v14l11-7z" />
    </svg>
  );
}

export function RepeatIcon({ one }: { one: boolean }) {
  return (
    <>
      <svg className="mode-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4 10V8a3 3 0 0 1 3-3h13m-4-4 4 4-4 4M20 14v2a3 3 0 0 1-3 3H4m4-4-4 4 4 4" />
      </svg>
      {one && (
        <span className="repeat-one" aria-hidden="true">
          1
        </span>
      )}
    </>
  );
}
