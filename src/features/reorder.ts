import { useEffect, useRef, useState } from 'react';
import type { MouseEvent, PointerEvent, RefObject } from 'react';

/** Pointer travel in px before a press becomes a drag; less stays a click. */
export const DRAG_FROM = 6;

/** A drag in progress: the item's index and the index it would land at. */
export type Drag = { from: number; over: number };

type Press = {
  index: number;
  y: number;
  pointer: number;
  dragging: boolean;
};

/**
 * The item a pointer at `y` is over, given the items' boxes top to bottom: above the first is
 * the first, below the last is the last.
 */
export function overIndex(
  y: number,
  boxes: { top: number; bottom: number }[],
): number {
  const index = boxes.findIndex((box) => y < box.bottom);
  return index < 0 ? boxes.length - 1 : index;
}

export type Reorder = {
  drag: Drag | null;
  /** Props for item `index`: the element that can be dragged. */
  bind: (index: number) => {
    'data-reorder': number;
    onPointerDown: (event: PointerEvent<HTMLElement>) => void;
    onPointerMove: (event: PointerEvent<HTMLElement>) => void;
    onPointerUp: (event: PointerEvent<HTMLElement>) => void;
    onPointerCancel: () => void;
    onClickCapture: (event: MouseEvent<HTMLElement>) => void;
  };
  /** `dragging` for the item moving, `drop-before`/`drop-after` where it would land. */
  classFor: (index: number) => string;
};

/**
 * Drag-to-reorder with the pointer (mouse, pen or touch), for items marked `data-reorder`
 * inside `container`. A press that moves less than `DRAG_FROM` stays a click; a drag never
 * clicks. Escape cancels. Pointer events rather than HTML drag and drop, which the desktop
 * WebView can hand to the system for file drops.
 */
export function useDragReorder<E extends HTMLElement>(
  container: RefObject<E | null>,
  onMove: (from: number, to: number) => void,
): Reorder {
  const [drag, setDrag] = useState<Drag | null>(null);
  const press = useRef<Press | null>(null);
  const swallowClick = useRef(false);

  const dragging = drag !== null;
  useEffect(() => {
    if (!dragging) return;
    const cancel = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      press.current = null;
      setDrag(null);
    };
    window.addEventListener('keydown', cancel, true);
    return () => window.removeEventListener('keydown', cancel, true);
  }, [dragging]);

  /** The item index under `y`, among the items rendered now (a long list renders a window). */
  function locate(y: number, fallback: number): number {
    const items = [
      ...(container.current?.querySelectorAll<HTMLElement>('[data-reorder]') ??
        []),
    ];
    if (items.length === 0) return fallback;
    const at = overIndex(
      y,
      items.map((item) => item.getBoundingClientRect()),
    );
    return Number(items[at]!.dataset.reorder ?? fallback);
  }

  return {
    drag,
    bind: (index) => ({
      'data-reorder': index,
      onPointerDown: (event) => {
        if (event.button !== 0) return;
        // A drag's click may never have come; this press starts afresh.
        swallowClick.current = false;
        press.current = {
          index,
          y: event.clientY,
          pointer: event.pointerId,
          dragging: false,
        };
      },
      onPointerMove: (event) => {
        const current = press.current;
        if (!current || current.pointer !== event.pointerId) return;
        if (!current.dragging) {
          if (Math.abs(event.clientY - current.y) < DRAG_FROM) return;
          current.dragging = true;
          event.currentTarget.setPointerCapture?.(event.pointerId);
        }
        const over = locate(event.clientY, current.index);
        setDrag((previous) =>
          previous?.from === current.index && previous.over === over
            ? previous
            : { from: current.index, over },
        );
      },
      onPointerUp: (event) => {
        const current = press.current;
        press.current = null;
        if (!current?.dragging) return;
        swallowClick.current = true;
        const over = locate(event.clientY, current.index);
        setDrag(null);
        if (over !== current.index) onMove(current.index, over);
      },
      onPointerCancel: () => {
        press.current = null;
        setDrag(null);
      },
      onClickCapture: (event) => {
        if (!swallowClick.current) return;
        swallowClick.current = false;
        event.preventDefault();
        event.stopPropagation();
      },
    }),
    classFor: (index) => {
      if (!drag) return '';
      if (index === drag.from) return 'dragging';
      if (index !== drag.over) return '';
      return drag.over < drag.from ? 'drop-before' : 'drop-after';
    },
  };
}
