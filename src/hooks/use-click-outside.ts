import { useEffect, useRef, type RefObject } from "react";

export function useClickOutside(
  ref: RefObject<HTMLElement | null>,
  callback: () => void,
  enabled = true,
  ignoreRef?: RefObject<HTMLElement | null>,
) {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    if (!enabled) return;
    const handle = (e: MouseEvent) => {
      const target = e.target as Node;
      if (ref.current && !ref.current.contains(target)) {
        if (ignoreRef?.current?.contains(target)) return;
        callbackRef.current();
      }
    };
    // Capture phase: runs before any descendant can stopPropagation on the
    // native event (e.g. React Flow's drag handlers), so clicks anywhere
    // outside the ref reliably close — regardless of what consumes the event.
    document.addEventListener("mousedown", handle, true);
    return () => document.removeEventListener("mousedown", handle, true);
  }, [enabled]); // ref + ignoreRef are stable RefObjects — not dependencies
}
