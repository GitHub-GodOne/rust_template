import type { ReactNode } from "react";

export function FinderWorkspace({
  title,
  icon,
  meta,
  status,
  statusAside,
  children,
}: {
  title: string;
  icon: ReactNode;
  meta?: ReactNode;
  status?: ReactNode;
  statusAside?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="finder-window" aria-label={title}>
      <header className="finder-window-titlebar">
        <div className="finder-window-title">
          <span className="finder-window-title-icon">{icon}</span>
          <strong>{title}</strong>
        </div>
        {meta ? <div className="finder-window-meta">{meta}</div> : null}
      </header>
      <div className="finder-window-body">{children}</div>
      {status || statusAside ? (
        <footer className="finder-window-statusbar">
          <span>{status}</span>
          <span>{statusAside}</span>
        </footer>
      ) : null}
    </section>
  );
}
