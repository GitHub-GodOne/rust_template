import { CloseOutlined } from "@ant-design/icons";
import { Button, Tooltip } from "antd";
import { useLocation, useNavigate } from "react-router-dom";
import { formatBytes } from "../components/admin/finder/finderModel";
import { t } from "../i18n";
import "../pages/system/remote-audio/remoteAudio.css";
import { useRemoteAudioStore } from "../stores/remoteAudio";

const PAGE_PATH = "/admin/system/remote-audio";

/** Header pill while 远程音频 plays on another page; opens it or stops. */
export function RemoteAudioIndicator() {
  const status = useRemoteAudioStore((state) => state.status);
  const bytes = useRemoteAudioStore((state) => state.bytesReceived);
  const stop = useRemoteAudioStore((state) => state.stop);
  const location = useLocation();
  const navigate = useNavigate();
  if (status === "idle" || location.pathname === PAGE_PATH) {
    return null;
  }
  return (
    <span className="remote-audio-indicator">
      <Tooltip title={t("正在收听远程音频，已用 {0}", formatBytes(bytes))}>
        <Button
          size="small"
          type="text"
          className="remote-audio-indicator-open"
          onClick={() => navigate(PAGE_PATH)}
        >
          <span className="remote-audio-indicator-bars" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <span className="remote-audio-indicator-label">{t("远程音频")}</span>
        </Button>
      </Tooltip>
      <Button
        size="small"
        type="text"
        className="remote-audio-indicator-stop"
        icon={<CloseOutlined />}
        aria-label={t("停止收听")}
        onClick={stop}
      />
    </span>
  );
}
