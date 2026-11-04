import { useQueryClient } from "@tanstack/react-query";
import { message, notification } from "antd";
import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { IM_CONVERSATIONS_KEY, IM_EVENT, type ImEvent } from "../api/admin/im";
import { fetchEventsTicket } from "../api/userNotifications";
import { t } from "../i18n";
import { messagePreview } from "../pages/system/im/imFormat";
import { useAuthStore } from "../stores/auth";
import { NOTIFICATION_FEED_KEY } from "./NotificationBell";

type PushedNotification = {
  title: string;
  content: string;
  level: string;
};

const MAX_RETRY_DELAY = 60_000;
const IM_PATH = "/admin/system/im";

/** Reconnect delay after the n-th failure: 2s, 4s, 8s … capped at 60s. */
export function retryDelay(attempt: number) {
  return Math.min(2000 * 2 ** Math.max(0, attempt), MAX_RETRY_DELAY);
}

/**
 * Keeps a server-sent event stream open while signed in: new notifications
 * pop up and refresh the bell, chat messages reach the 在线聊天 page (and
 * pop up elsewhere); a forced sign-out ends this page's session.
 */
export function useRealtimeEvents() {
  const accessToken = useAuthStore((state) => state.accessToken);
  const signOut = useAuthStore((state) => state.signOut);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  useEffect(() => {
    if (!accessToken || typeof EventSource === "undefined") return;
    let source: EventSource | null = null;
    let timer: number | undefined;
    let attempt = 0;
    let stopped = false;

    const schedule = () => {
      if (stopped) return;
      timer = window.setTimeout(connect, retryDelay(attempt));
      attempt += 1;
    };

    async function connect() {
      try {
        const ticket = await fetchEventsTicket();
        if (stopped) return;
        source = new EventSource(
          `/api/user/events?ticket=${encodeURIComponent(ticket)}`,
        );
        source.onopen = () => {
          attempt = 0;
        };
        source.addEventListener("notification", (event) => {
          const data = JSON.parse(
            (event as MessageEvent<string>).data,
          ) as PushedNotification;
          const kind =
            data.level === "error"
              ? "error"
              : data.level === "warning" || data.level === "warn"
                ? "warning"
                : data.level === "success"
                  ? "success"
                  : "info";
          notification[kind]({
            message: data.title,
            description: data.content,
            placement: "bottomRight",
          });
          void queryClient.invalidateQueries({
            queryKey: NOTIFICATION_FEED_KEY,
          });
        });
        source.addEventListener("im", (event) => {
          const data = JSON.parse(
            (event as MessageEvent<string>).data,
          ) as ImEvent;
          window.dispatchEvent(new CustomEvent(IM_EVENT, { detail: data }));
          void queryClient.invalidateQueries({
            queryKey: IM_CONVERSATIONS_KEY,
          });
          if (
            data.type === "message" &&
            !data.mine &&
            window.location.pathname !== IM_PATH
          ) {
            const key = `im-${data.conversation_id}`;
            notification.open({
              key,
              message: data.message.sender_name ?? t("新消息"),
              description: messagePreview(data.message, {
                kind: "direct",
                viewer_id: 0,
              }),
              placement: "bottomRight",
              onClick: () => {
                notification.destroy(key);
                navigate(`${IM_PATH}?c=${data.conversation_id}`);
              },
            });
          }
        });
        source.addEventListener("session_revoked", () => {
          stopped = true;
          source?.close();
          signOut();
          message.warning(t("你的登录已在其他地方被结束，请重新登录"));
          navigate("/login", { replace: true });
        });
        source.onerror = () => {
          // The ticket is single use: reconnect with a fresh one.
          source?.close();
          source = null;
          schedule();
        };
      } catch {
        schedule();
      }
    }

    void connect();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      source?.close();
    };
  }, [accessToken, navigate, queryClient, signOut]);
}
