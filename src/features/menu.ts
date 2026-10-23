import type { MouseEvent } from 'react';

/** Where a row's context menu opens, and the element to give focus back to. */
export type MenuPoint = { x: number; y: number; opener: HTMLElement };

/** One action in a row's context menu. */
export type MenuItem = {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /** It opens another menu in this one's place (focus moves there, not back to the row). */
  opensMenu?: boolean;
};

/** The pointer, or the row itself for the Menu key and Shift+F10 (they report 0, 0). */
export function menuPoint(event: MouseEvent<HTMLElement>): MenuPoint {
  const opener = event.currentTarget;
  if (event.clientX === 0 && event.clientY === 0) {
    const box = opener.getBoundingClientRect();
    return { x: box.left, y: box.bottom, opener };
  }
  return { x: event.clientX, y: event.clientY, opener };
}
