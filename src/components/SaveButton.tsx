import likeIcon from '../assets/brand/like-icon-like.svg?no-inline';
import likedIcon from '../assets/brand/like-icon-liked.svg?no-inline';

type Props = {
  saved: boolean;
  /** What's saved, for the label: a title, or empty for "this". */
  name: string;
  /** Where it's saved: "Liked Songs" for tracks, "Your Library" otherwise. */
  place: string;
  onToggle: () => void;
};

/** Spotify's own Like icons, as the Design Guidelines ask for saving to the library. */
export function SaveButton({ saved, name, place, onToggle }: Props) {
  const what = name ? `“${name}”` : '';
  return (
    <button
      className="like-button"
      aria-label={
        saved
          ? `Remove ${what || 'this'} from ${place}`
          : `${place === 'Liked Songs' ? 'Add' : 'Save'} ${what || 'this'} to ${place}`
      }
      onClick={onToggle}
    >
      <img src={saved ? likedIcon : likeIcon} alt="" />
    </button>
  );
}
