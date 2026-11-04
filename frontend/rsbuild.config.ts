import { defineConfig } from "@rsbuild/core";
import { pluginReact } from "@rsbuild/plugin-react";

// https://rsbuild.dev/guide/basic/configure-rsbuild
export default defineConfig({
  plugins: [pluginReact()],
  html: {
    favicon: "src/assets/favicon.ico",
    title: "Loco SaaS Starter",
    meta: {
      viewport:
        "width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no, viewport-fit=cover",
      // Home-screen web app. Without the manifest's `scope: "/"`, iOS scopes
      // the app to the folder of the page that was added and shows the
      // Safari bars as soon as navigation leaves it.
      "apple-mobile-web-app-capable": "yes",
      "mobile-web-app-capable": "yes",
      "apple-mobile-web-app-title": "后台管理",
      "apple-mobile-web-app-status-bar-style": "default",
    },
    tags: [
      { tag: "link", attrs: { rel: "manifest", href: "/manifest.webmanifest" } },
      {
        tag: "link",
        attrs: { rel: "apple-touch-icon", href: "/apple-touch-icon.png" },
      },
      {
        tag: "meta",
        attrs: {
          name: "theme-color",
          content: "#f7f5f1",
          media: "(prefers-color-scheme: light)",
        },
      },
      {
        tag: "meta",
        attrs: {
          name: "theme-color",
          content: "#141414",
          media: "(prefers-color-scheme: dark)",
        },
      },
    ],
  },
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:5150",
        changeOrigin: true,
        secure: false,
        ws: true,
      },
      "/api-docs": {
        target: "http://127.0.0.1:5150",
        changeOrigin: true,
        secure: false,
      },
      "/swagger-ui": {
        target: "http://127.0.0.1:5150",
        changeOrigin: true,
        secure: false,
      },
    },
  },
});
