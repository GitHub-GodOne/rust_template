import { LeftOutlined, RightOutlined } from "@ant-design/icons";
import { Button, Slider, Tooltip } from "antd";
import {
  type ReactNode,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

type TableScrollAreaProps = {
  children: ReactNode;
  contentWidth: number;
  className?: string;
  footer?: ReactNode;
  label?: string;
};

const END_GUTTER = 16;
const SHELL_BORDER_WIDTH = 2;

export function TableScrollArea({
  children,
  contentWidth,
  className,
  footer,
  label = "表格",
}: TableScrollAreaProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [scrollLeft, setScrollLeft] = useState(0);
  const [maxScroll, setMaxScroll] = useState(0);

  const updateMetrics = useCallback(() => {
    const viewport = viewportRef.current;
    if (!viewport) {
      return;
    }
    const nextMax = Math.max(0, viewport.scrollWidth - viewport.clientWidth);
    setMaxScroll(nextMax);
    setScrollLeft(Math.min(viewport.scrollLeft, nextMax));
  }, []);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) {
      return;
    }

    updateMetrics();
    const resizeObserver = new ResizeObserver(updateMetrics);
    resizeObserver.observe(viewport);
    const canvas = viewport.firstElementChild;
    if (canvas) {
      resizeObserver.observe(canvas);
    }

    const handleWheel = (event: WheelEvent) => {
      const horizontalDelta =
        Math.abs(event.deltaX) >= Math.abs(event.deltaY)
          ? event.deltaX
          : event.shiftKey
            ? event.deltaY
            : 0;
      const nextMax = Math.max(0, viewport.scrollWidth - viewport.clientWidth);
      if (horizontalDelta === 0 || nextMax <= 0) {
        return;
      }

      const unit =
        event.deltaMode === 1
          ? 24
          : event.deltaMode === 2
            ? viewport.clientWidth
            : 1;
      const nextPosition = Math.min(
        nextMax,
        Math.max(0, viewport.scrollLeft + horizontalDelta * unit),
      );
      event.preventDefault();
      viewport.scrollLeft = nextPosition;
      setScrollLeft(nextPosition);
    };

    viewport.addEventListener("wheel", handleWheel, { passive: false });
    return () => {
      resizeObserver.disconnect();
      viewport.removeEventListener("wheel", handleWheel);
    };
  }, [updateMetrics]);

  const setHorizontalPosition = (value: number) => {
    const viewport = viewportRef.current;
    if (!viewport) {
      return;
    }
    viewport.scrollLeft = value;
    setScrollLeft(value);
  };

  const moveHorizontalPosition = (direction: -1 | 1) => {
    const viewport = viewportRef.current;
    if (!viewport) {
      return;
    }
    const step = Math.max(240, Math.round(viewport.clientWidth * 0.72));
    viewport.scrollTo({
      left: Math.min(
        maxScroll,
        Math.max(0, viewport.scrollLeft + step * direction),
      ),
      behavior: "smooth",
    });
  };

  return (
    <section
      className={[
        "table-scroll-area",
        footer ? "table-scroll-area-with-footer" : undefined,
        className,
      ]
        .filter(Boolean)
        .join(" ")}
      aria-label={`${label}横向滚动区域`}
    >
      <div className="table-scroll-controls">
        <Tooltip title="向左滚动">
          <Button
            type="text"
            size="small"
            aria-label={`${label}向左滚动`}
            icon={<LeftOutlined />}
            disabled={scrollLeft <= 1}
            onClick={() => moveHorizontalPosition(-1)}
          />
        </Tooltip>
        <Slider
          min={0}
          max={Math.max(1, maxScroll)}
          value={Math.min(scrollLeft, maxScroll)}
          disabled={maxScroll <= 1}
          tooltip={{ formatter: null }}
          aria-label={`${label}横向滚动位置`}
          onChange={setHorizontalPosition}
        />
        <Tooltip title="向右滚动">
          <Button
            type="text"
            size="small"
            aria-label={`${label}向右滚动`}
            icon={<RightOutlined />}
            disabled={maxScroll <= 1 || scrollLeft >= maxScroll - 1}
            onClick={() => moveHorizontalPosition(1)}
          />
        </Tooltip>
      </div>
      <div
        ref={viewportRef}
        className="table-scroll-viewport"
        onScroll={(event) => setScrollLeft(event.currentTarget.scrollLeft)}
      >
        <div
          className="table-scroll-canvas"
          style={{ width: contentWidth + SHELL_BORDER_WIDTH + END_GUTTER }}
        >
          <div
            className="table-scroll-content"
            style={{ width: contentWidth + SHELL_BORDER_WIDTH }}
          >
            {children}
          </div>
        </div>
      </div>
      {footer ? <div className="table-scroll-footer">{footer}</div> : null}
    </section>
  );
}
