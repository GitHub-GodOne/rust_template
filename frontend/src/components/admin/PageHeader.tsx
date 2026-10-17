import type { ReactNode } from "react";

/**
 * Page heading. The visible title, description and breadcrumb are gone: the
 * selected menu item and the open tab already say where you are, so pages
 * get the full height for their content. The title stays available to screen
 * readers, and `extra` (page actions) still renders as a slim bar.
 */
export function PageHeader({
  title,
  extra,
}: {
  title: string;
  /** Kept for callers; no longer shown. */
  subtitle?: string;
  icon?: ReactNode;
  breadcrumb?: string[];
  extra?: ReactNode;
}) {
  return (
    <>
      <h1 className="visually-hidden">{title}</h1>
      {extra ? <div className="page-actions">{extra}</div> : null}
    </>
  );
}
