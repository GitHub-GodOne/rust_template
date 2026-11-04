import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type RemoteAudioQualityKey = "high" | "standard" | "low";

export type RemoteAudioQuality = {
  key: RemoteAudioQualityKey;
  sample_rate: number;
  channels: number;
  /** Bitrate while sound plays; silence costs nothing. */
  bitrate_kbps: number;
};

export type RemoteAudioStatus = {
  supported: boolean;
  /** `wasapi` (Windows) or `pulseaudio` (Linux). */
  backend?: string | null;
  device?: string | null;
  message?: string | null;
  qualities: RemoteAudioQuality[];
  default_quality: RemoteAudioQualityKey;
};

export type RemoteAudioSessionStatus = "live" | "ended" | "failed";

export type RemoteAudioSession = {
  id: number;
  user_id?: number | null;
  user_name?: string | null;
  user_email?: string | null;
  quality: RemoteAudioQualityKey;
  sample_rate: number;
  channels: number;
  /** Audio bytes sent from opening to closing (live: so far). */
  bytes_sent: number;
  /** Audio actually sent; silence is skipped. */
  audio_ms: number;
  duration_ms: number;
  status: RemoteAudioSessionStatus;
  close_reason?: string | null;
  error_message?: string | null;
  device?: string | null;
  client_ip?: string | null;
  user_agent?: string | null;
  started_at: string;
  ended_at?: string | null;
};

export type RemoteAudioSessionPage = {
  items: RemoteAudioSession[];
  page: number;
  page_size: number;
  total: number;
  /** Bytes of every session matching the filters. */
  total_bytes: number;
};

export type RemoteAudioSessionFilters = {
  page?: number;
  page_size?: number;
  keyword?: string;
  status?: RemoteAudioSessionStatus;
};

/** Messages on the listening socket besides the binary PCM chunks. */
export type RemoteAudioServerMessage =
  | {
      type: "format";
      session_id: number;
      sample_rate: number;
      channels: number;
      encoding: "pcm_s16le";
      chunk_ms: number;
      device: string;
    }
  | {
      type: "stats";
      bytes_sent: number;
      audio_ms: number;
      elapsed_ms: number;
    }
  | { type: "error"; message: string }
  | { type: "closed"; reason: string };

export const REMOTE_AUDIO_SESSIONS_KEY = "admin-remote-audio-sessions";

export async function fetchRemoteAudioStatus() {
  const response = await apiClient.get<ApiResponse<RemoteAudioStatus>>(
    "/admin/remote-audio/status",
  );
  return unwrap(response.data);
}

export async function createRemoteAudioTicket(quality: RemoteAudioQualityKey) {
  const response = await apiClient.post<
    ApiResponse<{ ticket: string; expires_at: string }>
  >("/admin/remote-audio/tickets", { quality });
  return unwrap(response.data);
}

export async function fetchRemoteAudioSessions(
  params: RemoteAudioSessionFilters,
) {
  const response = await apiClient.get<ApiResponse<RemoteAudioSessionPage>>(
    "/admin/remote-audio/sessions",
    { params },
  );
  return unwrap(response.data);
}

export function buildRemoteAudioWebSocketUrl(ticket: string) {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/admin/remote-audio/ws/${ticket}`;
}
