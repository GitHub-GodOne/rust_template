import { apiClient } from "./client";

function filenameFromDisposition(header?: string) {
  const match = /filename="?([^";]+)"?/i.exec(header ?? "");
  return match?.[1];
}

/** Downloads a CSV export endpoint as a file; returns the row count. */
export async function downloadCsv(
  path: string,
  params: Record<string, unknown>,
  fallbackName: string,
) {
  const response = await apiClient.get<Blob>(path, {
    params,
    responseType: "blob",
    timeout: 5 * 60_000,
  });
  const name =
    filenameFromDisposition(response.headers["content-disposition"]) ??
    fallbackName;
  const url = window.URL.createObjectURL(response.data);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  window.URL.revokeObjectURL(url);
  return Number(response.headers["x-total-count"] ?? 0);
}
