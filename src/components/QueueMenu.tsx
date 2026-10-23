import { useEffect, useRef } from 'react';
import type { MenuPoint } from '../features/menu';

type Props = {
  /** The track or episode, for the item's accessible name. */
  name: string;
  at: MenuPoint;
  onAdd: () => void;
  onClose: () => void;
};

/** Room kept for the menu at the window's right and bottom edges, in px. */
const MENU_WIDTH = 192;
const MENU_HEIGHT = 64;

/** A row's context menu: "Add to queue". It closes on Escape, Tab, a click elsewhere or a scroll. */
export function QueueMenu({ name, at, onAdd, onClose }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  const item = useRef<HTMLButtonElement>(null);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  }, [onClose]);

  useEffect(() => {
    item.current?.focus();
    const away = (event: Event) => {
      if (!menu.current?.contains(event.target as Node)) close.current();
    };
    const dismiss = () => close.current();
    document.addEventListener('mousedown', away, true);
    document.addEventListener('scroll', dismiss, true);
    window.addEventListener('blur', dismiss);
    window.addEventListener('resize', dismiss);
    return () => {
      document.removeEventListener('mousedown', away, true);
      document.removeEventListener('scroll', dismiss, true);
      window.removeEventListener('blur', dismiss);
      window.removeEventListener('resize', dismiss);
    };
  }, []);

  function leave(): void {
    onClose();
    at.opener.focus();
  }

  return (
    <div
      ref={menu}
      className="device-popover queue-menu"
      role="menu"
      aria-label={`Actions for ${name}`}
      style={{
        left: Math.max(0, Math.min(at.x, window.innerWidth - MENU_WIDTH)),
        top: Math.max(0, Math.min(at.y, window.innerHeight - MENU_HEIGHT)),
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape' || event.key === 'Tab') {
          event.preventDefault();
          leave();
        }
      }}
    >
      <button
        ref={item}
        className="device-option"
        role="menuitem"
        onClick={() => {
          onAdd();
          leave();
        }}
      >
        Add to queue
      </button>
    </div>
  );
}
