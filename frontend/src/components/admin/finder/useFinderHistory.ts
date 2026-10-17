import { useCallback, useEffect, useRef, useState } from "react";
import type { FinderView } from "./FinderShell";
import { type FinderHistory, historyMove, historyVisit } from "./finderModel";

/**
 * Back/forward navigation over opaque location keys. Every change of
 * `location` is recorded; `back`/`forward` call `navigate` with the stored key.
 */
export function useFinderHistory(
  location: string,
  navigate: (location: string) => void,
) {
  const [history, setHistory] = useState<FinderHistory>({
    entries: [location],
    index: 0,
  });
  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;

  useEffect(() => {
    setHistory((current) => historyVisit(current, location));
  }, [location]);

  const move = useCallback(
    (step: -1 | 1) => {
      const next = historyMove(history, step);
      if (next === history) return;
      setHistory(next);
      navigateRef.current(next.entries[next.index]);
    },
    [history],
  );

  return {
    canBack: history.index > 0,
    canForward: history.index < history.entries.length - 1,
    back: useCallback(() => move(-1), [move]),
    forward: useCallback(() => move(1), [move]),
  };
}

/** Grid/list choice remembered per page. */
export function useFinderView(
  storageKey: string,
  fallback: FinderView = "grid",
) {
  const key = `gpt-images-finder-view:${storageKey}`;
  const [view, setView] = useState<FinderView>(() => {
    const stored =
      typeof localStorage === "undefined" ? null : localStorage.getItem(key);
    return stored === "grid" || stored === "list" ? stored : fallback;
  });
  const update = useCallback(
    (next: FinderView) => {
      setView(next);
      localStorage.setItem(key, next);
    },
    [key],
  );
  return [view, update] as const;
}
