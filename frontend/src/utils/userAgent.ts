/** Short "Browser · OS" label for a session's user agent. */
export function describeUserAgent(userAgent?: string | null) {
  if (!userAgent) return "";
  const ua = userAgent;
  const browser =
    match(ua, /Edg(?:e|A|iOS)?\/(\d+)/, "Edge") ??
    match(ua, /OPR\/(\d+)/, "Opera") ??
    match(ua, /MicroMessenger\/(\d+)/, "WeChat") ??
    match(ua, /Firefox\/(\d+)/, "Firefox") ??
    match(ua, /CriOS\/(\d+)/, "Chrome") ??
    match(ua, /Chrome\/(\d+)/, "Chrome") ??
    match(ua, /Version\/(\d+)[\d.]* (?:Mobile\/\S+ )?Safari/, "Safari") ??
    (/curl\//i.test(ua) ? "curl" : null);
  const os = /Windows NT/.test(ua)
    ? "Windows"
    : /iPhone|iPad|iPod/.test(ua)
      ? "iOS"
      : /Android/.test(ua)
        ? "Android"
        : /Mac OS X|Macintosh/.test(ua)
          ? "macOS"
          : /Linux/.test(ua)
            ? "Linux"
            : null;
  const parts = [browser, os].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : ua.slice(0, 40);
}

function match(ua: string, pattern: RegExp, name: string) {
  const found = pattern.exec(ua);
  return found ? `${name} ${found[1]}` : null;
}
