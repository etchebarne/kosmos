import { useEffect, useRef, type RefObject } from "react";

export function useClickOutside(
  ref: RefObject<HTMLElement | null>,
  callback: () => void,
  enabled = true,
) {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    if (!enabled) return;
    const handle = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        callbackRef.current();
      }
    };
    // Capture phase: runs before any descendant can stopPropagation on the
    // native event (e.g. React Flow's drag handlers), so clicks anywhere
    // outside the ref reliably close — regardless of what consumes the event.
    document.addEventListener("mousedown", handle, true);
    return () => document.removeEventListener("mousedown", handle, true);
  }, [enabled]); // ref is a stable RefObject — not a dependency
}
