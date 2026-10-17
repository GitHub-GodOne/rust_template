import { type RefObject, useEffect, useRef } from "react";
import {
  LEFT_BUTTON,
  type Point,
  RIGHT_BUTTON,
  type TrackpadAction,
  TrackpadGesture,
} from "./vncInput";

type CaptureDocument = Document & { captureElement?: Element | null };

// MouseEvent.buttons bit for each MouseEvent.button value.
const BUTTON_BITS: Record<number, number> = { 0: 1, 1: 4, 2: 2 };

function remoteCanvas(stage: HTMLElement | null) {
  return stage?.querySelector<HTMLCanvasElement>(".vnc-viewer-host canvas");
}

function mouseInit(
  clientX: number,
  clientY: number,
  button: number,
  buttons: number,
) {
  return {
    bubbles: true,
    cancelable: true,
    view: window,
    clientX,
    clientY,
    button,
    buttons,
  } satisfies MouseEventInit;
}

/**
 * noVNC captures the pointer on mousedown and stops propagation of the
 * matching mouseup, so a synthetic release has to go through its window-level
 * capture proxy or the invisible capture overlay would swallow the next touch.
 */
function releaseAt(
  canvas: HTMLCanvasElement,
  clientX: number,
  clientY: number,
  button: number,
) {
  const init = mouseInit(clientX, clientY, button, 0);
  const captured = (document as CaptureDocument).captureElement === canvas;
  if (captured && !("setCapture" in canvas)) {
    window.dispatchEvent(new MouseEvent("mouseup", init));
    return;
  }
  canvas.dispatchEvent(new MouseEvent("mouseup", init));
  (document as Document & { releaseCapture?: () => void }).releaseCapture?.();
}

function pressAt(
  canvas: HTMLCanvasElement,
  clientX: number,
  clientY: number,
  button: number,
) {
  canvas.dispatchEvent(
    new MouseEvent(
      "mousedown",
      mouseInit(clientX, clientY, button, BUTTON_BITS[button] ?? 1),
    ),
  );
}

export function clickAt(
  canvas: HTMLCanvasElement,
  clientX: number,
  clientY: number,
  button: number,
) {
  canvas.dispatchEvent(
    new MouseEvent("mousemove", mouseInit(clientX, clientY, 0, 0)),
  );
  pressAt(canvas, clientX, clientY, button);
  releaseAt(canvas, clientX, clientY, button);
}

function touchPoints(list: TouchList): Point[] {
  return Array.from(list, (touch) => ({ x: touch.clientX, y: touch.clientY }));
}

type PointerModeOptions = {
  stageRef: RefObject<HTMLDivElement | null>;
  cursorRef: RefObject<HTMLSpanElement | null>;
  /** Only the visible, connected session handles input. */
  enabled: boolean;
  trackpad: boolean;
  rightClickArmed: boolean;
  onRightClickUsed: () => void;
};

/**
 * Touch/mouse handling layered over noVNC: a trackpad mode with a visible
 * cursor for touch screens, and a one-shot "next click is a right click".
 */
export function useVncPointerModes({
  stageRef,
  cursorRef,
  enabled,
  trackpad,
  rightClickArmed,
  onRightClickUsed,
}: PointerModeOptions) {
  const gestureRef = useRef<TrackpadGesture | null>(null);
  const armedRef = useRef(rightClickArmed);
  const usedRef = useRef(onRightClickUsed);
  armedRef.current = rightClickArmed;
  usedRef.current = onRightClickUsed;

  // Trackpad mode: one finger moves the cursor, taps click at the cursor.
  useEffect(() => {
    const stage = stageRef.current;
    const cursor = cursorRef.current;
    if (!enabled || !trackpad || !stage || !cursor) {
      return;
    }
    const gesture = gestureRef.current ?? new TrackpadGesture();
    gestureRef.current = gesture;
    let longPressTimer: number | undefined;
    let boundsReady = false;

    const placeCursor = () => {
      const canvas = remoteCanvas(stage);
      if (!canvas) {
        return;
      }
      const canvasBox = canvas.getBoundingClientRect();
      const stageBox = stage.getBoundingClientRect();
      cursor.style.transform = `translate3d(${canvasBox.left - stageBox.left + gesture.cursor.x}px, ${canvasBox.top - stageBox.top + gesture.cursor.y}px, 0)`;
      cursor.classList.toggle("is-dragging", gesture.isDragging);
      cursor.classList.add("is-visible");
    };

    const syncBounds = () => {
      const canvas = remoteCanvas(stage);
      if (!canvas) {
        return null;
      }
      const box = canvas.getBoundingClientRect();
      gesture.setBounds(box.width, box.height, !boundsReady);
      boundsReady = box.width > 0 && box.height > 0;
      return { canvas, box };
    };

    const apply = (actions: TrackpadAction[]) => {
      const target = syncBounds();
      if (!target) {
        return;
      }
      const { canvas, box } = target;
      for (const action of actions) {
        const clientX = box.left + action.x;
        const clientY = box.top + action.y;
        if (action.type === "move") {
          canvas.dispatchEvent(
            new MouseEvent(
              "mousemove",
              mouseInit(clientX, clientY, 0, action.buttons),
            ),
          );
        } else if (action.type === "down") {
          pressAt(canvas, clientX, clientY, action.button);
          navigator.vibrate?.(12);
        } else if (action.type === "up") {
          releaseAt(canvas, clientX, clientY, action.button);
        } else if (action.type === "click") {
          clickAt(canvas, clientX, clientY, action.button);
        } else {
          canvas.dispatchEvent(
            new WheelEvent("wheel", {
              ...mouseInit(clientX, clientY, 0, 0),
              deltaX: action.deltaX,
              deltaY: action.deltaY,
              deltaMode: WheelEvent.DOM_DELTA_PIXEL,
            }),
          );
        }
      }
      placeCursor();
    };

    const clearLongPress = () => {
      window.clearTimeout(longPressTimer);
      longPressTimer = undefined;
    };

    const swallow = (event: TouchEvent) => {
      // Keep noVNC's own touch gestures (direct touch) out of trackpad mode.
      event.preventDefault();
      event.stopPropagation();
    };

    const handleStart = (event: TouchEvent) => {
      if (!remoteCanvas(stage)) {
        return;
      }
      swallow(event);
      syncBounds();
      gesture.start(touchPoints(event.touches), event.timeStamp);
      clearLongPress();
      if (event.touches.length === 1) {
        longPressTimer = window.setTimeout(
          () => apply(gesture.longPress()),
          gesture.options.longPressMs,
        );
      }
      placeCursor();
    };
    const handleMove = (event: TouchEvent) => {
      swallow(event);
      apply(gesture.move(touchPoints(event.touches)));
      if (gesture.hasMoved) {
        clearLongPress();
      }
    };
    const handleEnd = (event: TouchEvent) => {
      swallow(event);
      if (event.touches.length === 0) {
        clearLongPress();
      }
      const armed = armedRef.current;
      const actions =
        event.type === "touchcancel"
          ? gesture.cancel()
          : gesture.end(
              touchPoints(event.touches),
              event.timeStamp,
              armed ? RIGHT_BUTTON : LEFT_BUTTON,
            );
      apply(actions);
      if (
        armed &&
        actions.some(
          (action) => action.type === "click" && action.button === RIGHT_BUTTON,
        )
      ) {
        usedRef.current();
      }
    };

    // A real mouse drives the remote pointer directly; hide the stale cursor.
    const handleMouse = (event: PointerEvent) => {
      if (event.pointerType === "mouse") {
        cursor.classList.remove("is-visible");
      }
    };

    const options = { capture: true, passive: false } as const;
    stage.addEventListener("pointermove", handleMouse);
    stage.addEventListener("touchstart", handleStart, options);
    stage.addEventListener("touchmove", handleMove, options);
    stage.addEventListener("touchend", handleEnd, options);
    stage.addEventListener("touchcancel", handleEnd, options);
    const handleResize = () => {
      syncBounds();
      placeCursor();
    };
    window.addEventListener("resize", handleResize);
    // The canvas also moves or rescales without a window resize (layout,
    // remote resolution changes), so follow it directly.
    const observer = new ResizeObserver(handleResize);
    observer.observe(stage);
    const observedCanvas = remoteCanvas(stage);
    if (observedCanvas) {
      observer.observe(observedCanvas);
    }
    const frame = requestAnimationFrame(handleResize);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      clearLongPress();
      apply(gesture.cancel());
      cursor.classList.remove("is-visible", "is-dragging");
      stage.removeEventListener("pointermove", handleMouse);
      stage.removeEventListener("touchstart", handleStart, options);
      stage.removeEventListener("touchmove", handleMove, options);
      stage.removeEventListener("touchend", handleEnd, options);
      stage.removeEventListener("touchcancel", handleEnd, options);
      window.removeEventListener("resize", handleResize);
    };
  }, [cursorRef, enabled, stageRef, trackpad]);

  // Direct mode with right click armed: the next tap or click becomes a right click.
  useEffect(() => {
    const stage = stageRef.current;
    if (!enabled || trackpad || !rightClickArmed || !stage) {
      return;
    }
    let touchStart: Point | null = null;

    const fire = (clientX: number, clientY: number) => {
      const canvas = remoteCanvas(stage);
      if (canvas) {
        clickAt(canvas, clientX, clientY, RIGHT_BUTTON);
      }
      usedRef.current();
    };
    const handleTouchStart = (event: TouchEvent) => {
      if (!remoteCanvas(stage) || event.touches.length !== 1) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      touchStart = { x: event.touches[0].clientX, y: event.touches[0].clientY };
    };
    const handleTouchEnd = (event: TouchEvent) => {
      if (!touchStart) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const point = touchStart;
      touchStart = null;
      fire(point.x, point.y);
    };
    const handleMouseDown = (event: MouseEvent) => {
      if (
        event.button !== LEFT_BUTTON ||
        !event.isTrusted ||
        !remoteCanvas(stage)
      ) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
    };
    const handleMouseUp = (event: MouseEvent) => {
      if (
        event.button !== LEFT_BUTTON ||
        !event.isTrusted ||
        !remoteCanvas(stage)
      ) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      fire(event.clientX, event.clientY);
    };

    const options = { capture: true, passive: false } as const;
    stage.addEventListener("touchstart", handleTouchStart, options);
    stage.addEventListener("touchend", handleTouchEnd, options);
    stage.addEventListener("mousedown", handleMouseDown, options);
    stage.addEventListener("mouseup", handleMouseUp, options);
    return () => {
      stage.removeEventListener("touchstart", handleTouchStart, options);
      stage.removeEventListener("touchend", handleTouchEnd, options);
      stage.removeEventListener("mousedown", handleMouseDown, options);
      stage.removeEventListener("mouseup", handleMouseUp, options);
    };
  }, [enabled, rightClickArmed, stageRef, trackpad]);
}
