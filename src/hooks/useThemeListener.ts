import { useEffect } from "react";

/**
 * Runs the provided callback whenever the app theme changes.
 * The callback must be stable (wrapped in useCallback) to avoid
 * re-subscribing on every render.
 */
export function useThemeListener(handler: () => void) {
  useEffect(() => {
    window.addEventListener("theme-changed", handler);
    return () => window.removeEventListener("theme-changed", handler);
  }, [handler]);
}
