import Guacamole from "guacamole-common-js";
import type { PointerSurface } from "../vnc/useVncPointerModes";

/** Wheel pixels that make one Guacamole scroll-button click. */
const SCROLL_STEP = 40;

/**
 * Drives a Guacamole client from the shared VNC pointer modes: positions are
 * sent relative to the scaled display element, buttons are tracked here.
 */
export function guacamoleSurface(
  client: () => Guacamole.Client | undefined,
): PointerSurface {
  const pressed = { left: false, middle: false, right: false };
  let scrollY = 0;

  const send = (
    target: HTMLElement,
    x: number,
    y: number,
    up = false,
    down = false,
  ) => {
    const box = target.getBoundingClientRect();
    client()?.sendMouseState(
      new Guacamole.Mouse.State(
        x - box.left,
        y - box.top,
        pressed.left,
        pressed.middle,
        pressed.right,
        up,
        down,
      ),
      true,
    );
  };
  const setButton = (button: number, down: boolean) => {
    if (button === 2) pressed.right = down;
    else if (button === 1) pressed.middle = down;
    else pressed.left = down;
  };

  return {
    element: (stage) => stage.querySelector<HTMLElement>(".rdp-desktop > div"),
    move: (target, x, y, buttons) => {
      pressed.left = (buttons & 1) !== 0;
      pressed.right = (buttons & 2) !== 0;
      pressed.middle = (buttons & 4) !== 0;
      send(target, x, y);
    },
    press: (target, x, y, button) => {
      setButton(button, true);
      send(target, x, y);
    },
    release: (target, x, y, button) => {
      setButton(button, false);
      send(target, x, y);
    },
    click: (target, x, y, button) => {
      send(target, x, y);
      setButton(button, true);
      send(target, x, y);
      setButton(button, false);
      send(target, x, y);
    },
    scroll: (target, x, y, _deltaX, deltaY) => {
      scrollY += deltaY;
      while (Math.abs(scrollY) >= SCROLL_STEP) {
        const down = scrollY > 0;
        send(target, x, y, !down, down);
        send(target, x, y);
        scrollY -= down ? SCROLL_STEP : -SCROLL_STEP;
      }
    },
  };
}

/**
 * Handles touch input through the same transformed-bounds mapping as the
 * physical mouse. Guacamole.Mouse.Touchscreen uses offsetLeft/offsetTop and
 * can therefore drift after viewport scaling or panning.
 */
export function attachGuacamoleInput(
  target: HTMLElement,
  surface: PointerSurface,
  onFocus: () => void,
): () => void {
  type TouchState = {
    x: number;
    y: number;
    moved: boolean;
    multi: boolean;
    rightClicked: boolean;
    timer?: number;
  };

  let touch: TouchState | undefined;
  const clearTimer = () => {
    if (touch?.timer !== undefined) {
      window.clearTimeout(touch.timer);
      touch.timer = undefined;
    }
  };
  const point = (value: Touch) => ({ x: value.clientX, y: value.clientY });
  const mouseMove = (event: MouseEvent) => {
    surface.move(target, event.clientX, event.clientY, event.buttons);
  };
  const mouseDown = (event: MouseEvent) => {
    if (event.button < 0 || event.button > 2) return;
    event.preventDefault();
    onFocus();
    surface.press(target, event.clientX, event.clientY, event.button);
  };
  const mouseUp = (event: MouseEvent) => {
    if (event.button < 0 || event.button > 2) return;
    event.preventDefault();
    surface.release(target, event.clientX, event.clientY, event.button);
  };
  const contextMenu = (event: MouseEvent) => event.preventDefault();
  const wheel = (event: WheelEvent) => {
    if (event.ctrlKey) return;
    event.preventDefault();
    surface.scroll(
      target,
      event.clientX,
      event.clientY,
      event.deltaX,
      event.deltaY,
    );
  };
  const start = (event: TouchEvent) => {
    event.preventDefault();
    onFocus();
    if (event.touches.length !== 1) {
      if (touch) touch.multi = true;
      clearTimer();
      return;
    }
    const value = point(event.touches[0]);
    touch = { ...value, moved: false, multi: false, rightClicked: false };
    touch.timer = window.setTimeout(() => {
      if (!touch || touch.moved || touch.multi || touch.rightClicked) return;
      surface.click(target, touch.x, touch.y, 2);
      touch.rightClicked = true;
    }, 500);
  };
  const move = (event: TouchEvent) => {
    event.preventDefault();
    if (!touch) return;
    if (event.touches.length !== 1) {
      touch.multi = true;
      clearTimer();
      return;
    }
    const value = point(event.touches[0]);
    if (Math.hypot(value.x - touch.x, value.y - touch.y) > 8) {
      touch.moved = true;
      clearTimer();
    }
  };
  const end = (event: TouchEvent) => {
    event.preventDefault();
    if (!touch) return;
    const state = touch;
    clearTimer();
    const changed = event.changedTouches[0];
    if (
      changed &&
      event.touches.length === 0 &&
      !state.moved &&
      !state.multi &&
      !state.rightClicked
    ) {
      const value = point(changed);
      surface.click(target, value.x, value.y, 0);
    }
    touch = undefined;
  };
  const options = { capture: true, passive: false } as const;
  target.addEventListener("mousemove", mouseMove);
  target.addEventListener("mousedown", mouseDown, options);
  target.addEventListener("mouseup", mouseUp, options);
  target.addEventListener("contextmenu", contextMenu, options);
  target.addEventListener("wheel", wheel, options);
  target.addEventListener("touchstart", start, options);
  target.addEventListener("touchmove", move, options);
  target.addEventListener("touchend", end, options);
  target.addEventListener("touchcancel", end, options);
  return () => {
    clearTimer();
    target.removeEventListener("mousemove", mouseMove);
    target.removeEventListener("mousedown", mouseDown, options);
    target.removeEventListener("mouseup", mouseUp, options);
    target.removeEventListener("contextmenu", contextMenu, options);
    target.removeEventListener("wheel", wheel, options);
    target.removeEventListener("touchstart", start, options);
    target.removeEventListener("touchmove", move, options);
    target.removeEventListener("touchend", end, options);
    target.removeEventListener("touchcancel", end, options);
  };
}
