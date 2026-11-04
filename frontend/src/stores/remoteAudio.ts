import { create } from "zustand";
import {
  type RemoteAudioQualityKey,
  type RemoteAudioServerMessage,
  buildRemoteAudioWebSocketUrl,
  createRemoteAudioTicket,
} from "../api/admin/remoteAudio";
import { t } from "../i18n";

/**
 * 远程音频 player. The engine lives outside React so listening continues
 * while the user moves to other pages (e.g. VNC); the header shows it.
 *
 * Phones suspend plain Web Audio once the screen turns off or another app
 * comes to the front, so the decoded sound is played through an `<audio>`
 * element instead (a `MediaStream` from the audio graph): the system then
 * treats it as media playback, keeps it going in the background and shows
 * lock-screen controls (Media Session). A connection lost meanwhile is
 * opened again on its own.
 */
export type RemoteAudioPlayerStatus = "idle" | "connecting" | "playing";

type RemoteAudioState = {
  status: RemoteAudioPlayerStatus;
  quality?: RemoteAudioQualityKey;
  sessionId?: number;
  device?: string;
  /** Set when the last session ended with a problem. */
  error?: string;
  /** Set while a dropped connection is being opened again. */
  reconnectAttempt?: number;
  startedAt?: number;
  /** Audio bytes received since listening started (counted by the browser). */
  bytesReceived: number;
  /** Bytes the server reports as sent on the current connection. */
  serverBytes: number;
  /** Bitrate over the last few seconds. */
  bitrateKbps: number;
  /** Peak level 0..1 of what played recently. */
  level: number;
  volume: number;
  muted: boolean;
  start: (quality: RemoteAudioQualityKey) => Promise<void>;
  stop: () => void;
  setVolume: (volume: number) => void;
  setMuted: (muted: boolean) => void;
};

const VOLUME_KEY = "gpt-images-admin-remote-audio-volume";
/** Audio queued ahead of playback to ride out network jitter. */
const JITTER_SECONDS = 0.15;
/** More queued audio than this is dropped to keep the delay short. */
const MAX_DELAY_SECONDS = 0.8;
const UI_REFRESH_MS = 500;
const BITRATE_WINDOW_MS = 3000;

function loadVolume() {
  try {
    const value = Number(localStorage.getItem(VOLUME_KEY));
    return Number.isFinite(value) && value > 0 && value <= 1 ? value : 0.8;
  } catch {
    return 0.8;
  }
}

/** 1 s, 2 s, 4 s … at most 10 s between attempts. */
const reconnectDelay = (attempt: number) =>
  Math.min(1000 * 2 ** Math.max(0, attempt - 1), 10_000);

type Format = { sampleRate: number; channels: number };

/** Sound output that outlives reconnects: graph, gain and media element. */
type Output = {
  context: AudioContext;
  gain: GainNode;
  /** Plays the graph as media so it keeps running in the background. */
  element?: HTMLAudioElement;
};

type Engine = {
  output: Output;
  quality: RemoteAudioQualityKey;
  socket?: WebSocket;
  format?: Format;
  nextTime: number;
  bytes: number;
  /** [timestamp, total bytes] samples for the bitrate. */
  history: [number, number][];
  peak: number;
  timer: number;
  reconnectTimer?: number;
  attempt: number;
  /** Stopped by the user (or a new session); no reconnecting. */
  stopping: boolean;
};

let engine: Engine | null = null;

const closeReasonMessage = (reason: string) =>
  reason === "user_disabled"
    ? t("账号已被停用，收听已结束")
    : t("管理员已断开这次收听");

/** Must run inside the click that starts listening (autoplay rules). */
function openOutput(volume: number, muted: boolean): Output {
  const context = new AudioContext({ latencyHint: "interactive" });
  void context.resume();
  const gain = context.createGain();
  gain.gain.value = muted ? 0 : volume;
  let element: HTMLAudioElement | undefined;
  try {
    const destination = context.createMediaStreamDestination();
    gain.connect(destination);
    element = new Audio();
    element.setAttribute("playsinline", "");
    element.srcObject = destination.stream;
    void element.play().catch(() => {
      // Media elements refused: fall back to the plain graph output.
      gain.disconnect();
      gain.connect(context.destination);
    });
  } catch {
    // No MediaStream output (old browser): plain Web Audio, foreground only.
    element = undefined;
    gain.connect(context.destination);
  }
  return { context, gain, element };
}

function closeOutput(output: Output) {
  if (output.element) {
    output.element.pause();
    output.element.srcObject = null;
  }
  void output.context.close().catch(() => undefined);
}

/** Wakes playback the system paused (screen lock, call, tab switch). */
function resumeOutput(output: Output) {
  if (output.context.state !== "running" && output.context.state !== "closed") {
    void output.context.resume().catch(() => undefined);
  }
  if (output.element?.paused) {
    void output.element.play().catch(() => undefined);
  }
}

export const useRemoteAudioStore = create<RemoteAudioState>((set, get) => {
  const media =
    typeof navigator !== "undefined" ? navigator.mediaSession : undefined;

  const showMediaSession = (device?: string) => {
    if (!media) return;
    try {
      media.metadata = new MediaMetadata({
        title: t("远程音频"),
        artist: device || t("本机声音"),
      });
      media.playbackState = "playing";
      media.setActionHandler("pause", () => teardown());
      media.setActionHandler("stop", () => teardown());
      media.setActionHandler("play", () => {
        const quality = get().quality;
        if (quality && !engine)
          void get()
            .start(quality)
            .catch(() => {});
      });
    } catch {
      // Media Session is optional.
    }
  };

  const hideMediaSession = () => {
    if (!media) return;
    try {
      media.playbackState = "none";
      media.metadata = null;
      for (const action of ["pause", "stop", "play"] as const) {
        media.setActionHandler(action, null);
      }
    } catch {
      // Media Session is optional.
    }
  };

  const onVisible = () => {
    if (document.visibilityState === "visible" && engine) {
      resumeOutput(engine.output);
    }
  };

  const closeSocket = (current: Engine) => {
    const socket = current.socket;
    current.socket = undefined;
    if (!socket) return;
    socket.onmessage = null;
    socket.onclose = null;
    if (
      socket.readyState === WebSocket.OPEN ||
      socket.readyState === WebSocket.CONNECTING
    ) {
      socket.close();
    }
  };

  const teardown = (error?: string) => {
    const current = engine;
    engine = null;
    if (current) {
      current.stopping = true;
      window.clearInterval(current.timer);
      window.clearTimeout(current.reconnectTimer);
      closeSocket(current);
      closeOutput(current.output);
    }
    document.removeEventListener("visibilitychange", onVisible);
    hideMediaSession();
    set({
      status: "idle",
      level: 0,
      bitrateKbps: 0,
      reconnectAttempt: undefined,
      ...(error ? { error } : {}),
    });
  };

  const play = (current: Engine, data: ArrayBuffer) => {
    const format = current.format;
    if (!format) return;
    current.bytes += data.byteLength;
    const { context, gain } = current.output;
    const samples = new Int16Array(data);
    const frames = Math.floor(samples.length / format.channels);
    if (frames === 0) return;
    const buffer = context.createBuffer(
      format.channels,
      frames,
      format.sampleRate,
    );
    let peak = 0;
    for (let channel = 0; channel < format.channels; channel += 1) {
      const output = buffer.getChannelData(channel);
      for (let frame = 0; frame < frames; frame += 1) {
        const value = samples[frame * format.channels + channel] / 32768;
        output[frame] = value;
        const magnitude = Math.abs(value);
        if (magnitude > peak) peak = magnitude;
      }
    }
    current.peak = Math.max(current.peak, peak);
    const now = context.currentTime;
    // After a pause (silence is not sent) or a stall, start a fresh buffer;
    // a growing backlog is dropped so the delay stays short.
    if (
      current.nextTime < now + 0.02 ||
      current.nextTime > now + MAX_DELAY_SECONDS
    ) {
      current.nextTime = now + JITTER_SECONDS;
    }
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.connect(gain);
    source.start(current.nextTime);
    current.nextTime += buffer.duration;
  };

  const handleMessage = (current: Engine, text: string) => {
    let message: RemoteAudioServerMessage;
    try {
      message = JSON.parse(text) as RemoteAudioServerMessage;
    } catch {
      return;
    }
    switch (message.type) {
      case "format":
        current.format = {
          sampleRate: message.sample_rate,
          channels: Math.max(1, message.channels),
        };
        current.attempt = 0;
        set({
          status: "playing",
          sessionId: message.session_id,
          device: message.device,
          reconnectAttempt: undefined,
          ...(get().startedAt ? {} : { startedAt: Date.now() }),
        });
        showMediaSession(message.device);
        break;
      case "stats":
        set({ serverBytes: message.bytes_sent });
        break;
      case "error":
        teardown(message.message);
        break;
      case "closed":
        teardown(closeReasonMessage(message.reason));
        break;
    }
  };

  /** Opens (again) the listening socket of `current`. */
  const connect = async (current: Engine) => {
    let ticket: string;
    try {
      ticket = (await createRemoteAudioTicket(current.quality)).ticket;
    } catch (error) {
      if (engine !== current) return;
      if (current.attempt > 0) {
        scheduleReconnect(current);
        return;
      }
      teardown(error instanceof Error ? error.message : t("连接失败"));
      throw error;
    }
    if (engine !== current) return;
    const socket = new WebSocket(buildRemoteAudioWebSocketUrl(ticket));
    socket.binaryType = "arraybuffer";
    current.socket = socket;
    current.format = undefined;
    current.nextTime = 0;
    socket.onmessage = (event) => {
      if (engine !== current || current.socket !== socket) return;
      if (typeof event.data === "string") {
        handleMessage(current, event.data);
      } else {
        play(current, event.data as ArrayBuffer);
      }
    };
    socket.onclose = () => {
      if (engine !== current || current.socket !== socket) return;
      current.socket = undefined;
      // Dropped (network change, phone in the background): open it again.
      scheduleReconnect(current);
    };
  };

  const scheduleReconnect = (current: Engine) => {
    if (current.stopping || engine !== current) return;
    current.attempt += 1;
    set({ status: "connecting", reconnectAttempt: current.attempt });
    window.clearTimeout(current.reconnectTimer);
    current.reconnectTimer = window.setTimeout(() => {
      if (engine !== current) return;
      resumeOutput(current.output);
      void connect(current).catch(() => {});
    }, reconnectDelay(current.attempt));
  };

  return {
    status: "idle",
    bytesReceived: 0,
    serverBytes: 0,
    bitrateKbps: 0,
    level: 0,
    volume: loadVolume(),
    muted: false,

    start: async (quality) => {
      teardown();
      // Created inside the click so browsers allow it to play.
      const { volume, muted } = get();
      const output = openOutput(volume, muted);
      set({
        status: "connecting",
        quality,
        error: undefined,
        sessionId: undefined,
        device: undefined,
        startedAt: undefined,
        reconnectAttempt: undefined,
        bytesReceived: 0,
        serverBytes: 0,
      });
      const current: Engine = {
        output,
        quality,
        nextTime: 0,
        bytes: 0,
        history: [],
        peak: 0,
        attempt: 0,
        stopping: false,
        timer: window.setInterval(() => {
          const now = Date.now();
          current.history.push([now, current.bytes]);
          while (
            current.history.length > 1 &&
            now - current.history[0][0] > BITRATE_WINDOW_MS
          ) {
            current.history.shift();
          }
          const [since, bytesThen] = current.history[0];
          const seconds = (now - since) / 1000;
          set({
            bytesReceived: current.bytes,
            bitrateKbps:
              seconds > 0
                ? ((current.bytes - bytesThen) * 8) / 1000 / seconds
                : 0,
            level: current.peak,
          });
          current.peak *= 0.4;
        }, UI_REFRESH_MS),
      };
      engine = current;
      // The system may pause playback (call, screen lock); pick it up again.
      output.context.onstatechange = () => {
        if (engine === current && document.visibilityState === "visible") {
          resumeOutput(output);
        }
      };
      document.addEventListener("visibilitychange", onVisible);
      await connect(current);
    },

    stop: () => teardown(),

    setVolume: (volume) => {
      try {
        localStorage.setItem(VOLUME_KEY, String(volume));
      } catch {
        // Only a convenience.
      }
      if (engine && !get().muted) engine.output.gain.gain.value = volume;
      set({ volume });
    },

    setMuted: (muted) => {
      if (engine) engine.output.gain.gain.value = muted ? 0 : get().volume;
      set({ muted });
    },
  };
});
