import type { IDisposable, Terminal } from "@xterm/xterm";
import { useCallback, useEffect, useRef, useState } from "react";
import { t } from "../../../i18n";

type ScrollState = {
  /** First line shown. */
  viewportY: number;
  /** Lines above the screen; 0 when there is nothing to scroll. */
  baseY: number;
  rows: number;
};

/** Thumb never gets shorter than this, so a long history stays grabbable. */
const MIN_THUMB_PX = 36;

/** Id of the element holding the terminals, which the scrollbar controls. */
export const SSH_TERMINAL_STAGE_ID = "ssh-terminal-stage";

/**
 * A slim draggable scrollbar for the phone terminal: touch screens only
 * get an indicator from the browser, and swiping through a long history is
 * slow. Drag the thumb to move through the scrollback, or tap the track to
 * jump there. Hidden while there is nothing to scroll (also in full-screen
 * programs such as vim).
 */
export function TerminalScrollbar({
  getTerminal,
}: {
  /** The active session's terminal; may not exist yet on the first render. */
  getTerminal: () => Terminal | undefined;
}) {
  const [terminal, setTerminal] = useState<Terminal>();
  const [state, setState] = useState<ScrollState>({
    viewportY: 0,
    baseY: 0,
    rows: 0,
  });
  const [dragging, setDragging] = useState(false);
  const [trackHeight, setTrackHeight] = useState(0);
  const trackRef = useRef<HTMLDivElement | null>(null);
  const observerRef = useRef<ResizeObserver | null>(null);
  const grabRef = useRef(0);
  const frameRef = useRef(0);

  // The track's height decides the thumb's size; it changes with the
  // keyboard and rotation.
  const bindTrack = useCallback((track: HTMLDivElement | null) => {
    observerRef.current?.disconnect();
    observerRef.current = null;
    trackRef.current = track;
    if (!track) return;
    setTrackHeight(track.clientHeight);
    if (typeof ResizeObserver !== "undefined") {
      observerRef.current = new ResizeObserver(() =>
        setTrackHeight(track.clientHeight),
      );
      observerRef.current.observe(track);
    }
  }, []);

  // The terminal is created after the page renders; look again shortly.
  useEffect(() => {
    const found = getTerminal();
    if (found) {
      setTerminal(found);
      return;
    }
    const timer = window.setInterval(() => {
      const next = getTerminal();
      if (next) {
        setTerminal(next);
        window.clearInterval(timer);
      }
    }, 250);
    return () => window.clearInterval(timer);
  }, [getTerminal]);

  useEffect(() => {
    if (!terminal) return;
    const update = () => {
      window.cancelAnimationFrame(frameRef.current);
      frameRef.current = window.requestAnimationFrame(() => {
        const buffer = terminal.buffer.active;
        setState((current) =>
          current.viewportY === buffer.viewportY &&
          current.baseY === buffer.baseY &&
          current.rows === terminal.rows
            ? current
            : {
                viewportY: buffer.viewportY,
                baseY: buffer.baseY,
                rows: terminal.rows,
              },
        );
      });
    };
    update();
    const disposables: IDisposable[] = [
      terminal.onScroll(update),
      terminal.onWriteParsed(update),
      terminal.onResize(update),
      terminal.buffer.onBufferChange(update),
    ];
    return () => {
      window.cancelAnimationFrame(frameRef.current);
      for (const disposable of disposables) disposable.dispose();
    };
  }, [terminal]);

  if (!terminal || state.baseY <= 0) {
    return null;
  }

  const total = state.baseY + state.rows;
  const thumbHeight = Math.max(
    MIN_THUMB_PX,
    trackHeight * (state.rows / total),
  );
  const travel = Math.max(1, trackHeight - thumbHeight);
  const thumbTop = (state.viewportY / state.baseY) * travel;

  /** Scrolls so the thumb's top sits at `offset` px down the track. */
  const scrollToOffset = (offset: number) => {
    const ratio = Math.min(1, Math.max(0, offset / travel));
    terminal.scrollToLine(Math.round(ratio * state.baseY));
  };

  const offsetIn = (clientY: number) =>
    clientY - (trackRef.current?.getBoundingClientRect().top ?? 0);

  return (
    <div
      ref={bindTrack}
      className={`ssh-terminal-scrollbar${dragging ? " is-dragging" : ""}`}
      role="scrollbar"
      aria-label={t("终端滚动条")}
      aria-orientation="vertical"
      aria-controls={SSH_TERMINAL_STAGE_ID}
      aria-valuemin={0}
      aria-valuemax={state.baseY}
      aria-valuenow={state.viewportY}
      tabIndex={-1}
      onPointerDown={(event) => {
        event.preventDefault();
        const offset = offsetIn(event.clientY);
        const onThumb = offset >= thumbTop && offset <= thumbTop + thumbHeight;
        // Grabbing the thumb keeps it under the finger; tapping the track
        // centres the thumb there.
        grabRef.current = onThumb ? offset - thumbTop : thumbHeight / 2;
        if (!onThumb) scrollToOffset(offset - grabRef.current);
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
      }}
      onPointerMove={(event) => {
        if (!dragging) return;
        scrollToOffset(offsetIn(event.clientY) - grabRef.current);
      }}
      onPointerUp={() => setDragging(false)}
      onPointerCancel={() => setDragging(false)}
    >
      <span
        className="ssh-terminal-scrollbar-thumb"
        style={{ height: thumbHeight, transform: `translateY(${thumbTop}px)` }}
      />
    </div>
  );
}
