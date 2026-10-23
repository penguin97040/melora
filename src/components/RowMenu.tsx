import { useEffect, useLayoutEffect, useRef } from 'react';
import type { KeyboardEvent } from 'react';
import type { MenuItem, MenuPoint } from '../features/menu';

type Props = {
  /** What the menu acts on, for its accessible name. */
  name: string;
  at: MenuPoint;
  items: MenuItem[];
  onClose: () => void;
};

/**
 * A row's context menu ("Add to queue", "Add to playlist…", moving and removing). ↑ and ↓ move
 * between items; it closes on Escape, Tab, a click elsewhere or a scroll, and focus goes back to
 * the row. An item that opens another menu (`opensMenu`) leaves focus to it.
 */
export function RowMenu({ name, at, items, onClose }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  }, [onClose]);

  useLayoutEffect(() => {
    // Kept inside the window: measured once it has its items.
    const element = menu.current;
    if (!element) return;
    const box = element.getBoundingClientRect();
    element.style.left = `${Math.max(0, Math.min(at.x, window.innerWidth - box.width))}px`;
    element.style.top = `${Math.max(0, Math.min(at.y, window.innerHeight - box.height))}px`;
  }, [at]);

  useEffect(() => {
    menu.current
      ?.querySelector<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')
      ?.focus();
    const away = (event: Event) => {
      if (!menu.current?.contains(event.target as Node)) close.current();
    };
    // Scrolling the menu's own list keeps it open.
    const dismiss = (event: Event) => {
      if (!menu.current?.contains(event.target as Node)) close.current();
    };
    const leave = () => close.current();
    document.addEventListener('mousedown', away, true);
    document.addEventListener('scroll', dismiss, true);
    window.addEventListener('blur', leave);
    window.addEventListener('resize', leave);
    return () => {
      document.removeEventListener('mousedown', away, true);
      document.removeEventListener('scroll', dismiss, true);
      window.removeEventListener('blur', leave);
      window.removeEventListener('resize', leave);
    };
  }, []);

  function leave(): void {
    onClose();
    at.opener.focus();
  }

  function step(event: KeyboardEvent<HTMLDivElement>): void {
    const buttons = [
      ...(menu.current?.querySelectorAll<HTMLButtonElement>(
        '[role="menuitem"]:not(:disabled)',
      ) ?? []),
    ];
    if (buttons.length === 0) return;
    const now = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? buttons.length - 1
          : (now + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) %
            buttons.length;
    buttons[next]!.focus();
  }

  return (
    <div
      ref={menu}
      className="device-popover queue-menu"
      role="menu"
      aria-label={`Actions for ${name}`}
      style={{ left: at.x, top: at.y }}
      onKeyDown={(event) => {
        if (event.key === 'Escape' || event.key === 'Tab') {
          event.preventDefault();
          leave();
        } else if (
          ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)
        ) {
          event.preventDefault();
          step(event);
        }
      }}
    >
      {items.map((item, i) => (
        <button
          // Two playlists can share a name.
          key={`${i}:${item.label}`}
          className="device-option"
          role="menuitem"
          disabled={item.disabled}
          onClick={() => {
            if (item.opensMenu) {
              item.onSelect();
              return;
            }
            leave();
            item.onSelect();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
