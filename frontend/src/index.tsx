import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";

import "antd/dist/reset.css";
import "./index.css";

function configureResponsiveViewport() {
  const mobileUserAgent =
    /Android|iPhone|iPad|iPod|IEMobile|Opera Mini|Mobile/i.test(
      navigator.userAgent,
    );
  const compactTouchScreen =
    navigator.maxTouchPoints > 0 &&
    Math.min(window.screen.width, window.screen.height) <= 1024;
  if (mobileUserAgent || !compactTouchScreen) {
    return;
  }

  const viewport = document.querySelector<HTMLMetaElement>(
    'meta[name="viewport"]',
  );
  if (viewport) {
    viewport.content = "width=1200, viewport-fit=cover";
  }
}

configureResponsiveViewport();

const root = document.getElementById("root");

if (!root) {
  throw new Error("No root element found");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
