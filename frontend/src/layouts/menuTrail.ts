/** Menu entries needed to describe a page's place in the navigation. */
export type TrailMenu = {
  key: string;
  label?: string;
  title?: string;
  path?: string;
  children?: TrailMenu[];
};

/**
 * Menu items from the top level down to the one whose path is `pathname`,
 * e.g. 系统管理 → 素材库. Empty when the page is not in the menu.
 */
export function findMenuTrail<T extends TrailMenu>(
  pathname: string,
  items: T[],
): T[] {
  for (const item of items) {
    if (item.path === pathname) return [item];
    if (item.children?.length) {
      const below = findMenuTrail(pathname, item.children as T[]);
      if (below.length) return [item, ...below];
    }
  }
  return [];
}

export function menuLabel(item: TrailMenu) {
  return item.label || item.title || item.key;
}
