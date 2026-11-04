import type Guacamole from "guacamole-common-js";

/** Largest zoom, in screen pixels per remote pixel. */
const MAX_SCALE = 3;
/** How close (px) the followed point may get to the edge of the view. */
const FOLLOW_MARGIN = 48;

export type ViewportState = {
  /** Screen pixels per remote pixel. */
  scale: number;
  /** Zoomed in past "whole desktop fits the window". */
  zoomed: boolean;
};

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);

/**
 * Shows the remote desktop at its own resolution inside the window, like
 * remote-control apps on phones: it starts fitted to the window and can be
 * zoomed around a point and panned; content smaller than the window stays
 * centered. Guacamole's built-in mouse handler derives coordinates from
 * offsetLeft/offsetTop, so the pan is applied through absolute left/top
 * positioning rather than a CSS transform. This keeps native mouse and touch
 * events aligned with the visible display (and with getBoundingClientRect()).
 */
export class RemoteViewport {
  /** Zoom relative to the fitted scale; 1 shows the whole desktop. */
  private zoom = 1;
  private x = 0;
  private y = 0;
  private reported?: ViewportState;

  constructor(
    private readonly container: HTMLElement,
    private readonly display: Guacamole.Display,
    private readonly onChange?: (state: ViewportState) => void,
  ) {
    const element = display.getElement();
    element.style.position = "absolute";
    element.style.left = "0";
    element.style.top = "0";
    element.style.transform = "none";
  }

  private get fitScale() {
    const width = this.display.getWidth();
    const height = this.display.getHeight();
    if (!width || !height || !this.container.clientWidth) {
      return 0;
    }
    return Math.min(
      this.container.clientWidth / width,
      this.container.clientHeight / height,
    );
  }

  get zoomed() {
    return this.zoom > 1.001;
  }

  /** Re-applies scale and pan; call after the window or remote size changes. */
  layout() {
    const fit = this.fitScale;
    if (!fit) {
      return;
    }
    this.zoom = clamp(this.zoom, 1, Math.max(1, MAX_SCALE / fit));
    const scale = fit * this.zoom;
    if (Math.abs(this.display.getScale() - scale) > 1e-6) {
      this.display.scale(scale);
    }
    const width = this.display.getWidth() * scale;
    const height = this.display.getHeight() * scale;
    const viewWidth = this.container.clientWidth;
    const viewHeight = this.container.clientHeight;
    this.x =
      width <= viewWidth
        ? (viewWidth - width) / 2
        : clamp(this.x, viewWidth - width, 0);
    this.y =
      height <= viewHeight
        ? (viewHeight - height) / 2
        : clamp(this.y, viewHeight - height, 0);
    // Do not use translate3d here. guacamole-common-js 1.5.0 translates
    // native events via offsetLeft/offsetTop (which do not include CSS
    // transforms), producing a cursor offset whenever the fitted desktop is
    // centered or the viewport is panned. Absolute positioning participates
    // in that calculation while retaining the same visual placement.
    const element = this.display.getElement();
    element.style.left = `${this.x}px`;
    element.style.top = `${this.y}px`;
    const state = { scale, zoomed: this.zoomed };
    if (
      !this.reported ||
      this.reported.zoomed !== state.zoomed ||
      Math.abs(this.reported.scale - state.scale) > 0.005
    ) {
      this.reported = state;
      this.onChange?.(state);
    }
  }

  /** Multiplies the zoom by `factor`, keeping the client point in place. */
  zoomAt(clientX: number, clientY: number, factor: number) {
    const fit = this.fitScale;
    if (!fit || !Number.isFinite(factor) || factor <= 0) {
      return;
    }
    const next = clamp(this.zoom * factor, 1, Math.max(1, MAX_SCALE / fit));
    const ratio = next / this.zoom;
    if (ratio === 1) {
      return;
    }
    const box = this.container.getBoundingClientRect();
    const pointX = clientX - box.left;
    const pointY = clientY - box.top;
    this.x = pointX - (pointX - this.x) * ratio;
    this.y = pointY - (pointY - this.y) * ratio;
    this.zoom = next;
    this.layout();
  }

  /** Moves the desktop by this many screen pixels. */
  panBy(deltaX: number, deltaY: number) {
    if (!this.zoomed) {
      return;
    }
    this.x += deltaX;
    this.y += deltaY;
    this.layout();
  }

  /** Pans just enough to keep the client point away from the view's edges. */
  follow(clientX: number, clientY: number, margin = FOLLOW_MARGIN) {
    if (!this.zoomed) {
      return;
    }
    const box = this.container.getBoundingClientRect();
    const inset = Math.min(margin, box.width / 4, box.height / 4);
    const deltaX =
      clientX < box.left + inset
        ? box.left + inset - clientX
        : clientX > box.right - inset
          ? box.right - inset - clientX
          : 0;
    const deltaY =
      clientY < box.top + inset
        ? box.top + inset - clientY
        : clientY > box.bottom - inset
          ? box.bottom - inset - clientY
          : 0;
    if (deltaX || deltaY) {
      this.panBy(deltaX, deltaY);
    }
  }

  /** Whole desktop in the window. */
  fit() {
    this.zoom = 1;
    this.layout();
  }

  /** One remote pixel per screen pixel, around the middle of the view. */
  actualSize() {
    const fit = this.fitScale;
    if (!fit) {
      return;
    }
    const box = this.container.getBoundingClientRect();
    this.zoomAt(
      box.left + box.width / 2,
      box.top + box.height / 2,
      1 / fit / this.zoom,
    );
  }
}
