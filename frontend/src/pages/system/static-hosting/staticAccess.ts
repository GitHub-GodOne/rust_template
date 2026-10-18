import { t } from "../../../i18n";
// Pure helpers for static-site password protection; kept import-free so the
// node tests can load them directly.

export type AccessRuleLike = { path: string };

function normalizePath(path: string) {
  return path
    .trim()
    .replace(/^\/+|\/+$/g, "")
    .toLowerCase();
}

/** Most specific rule that covers `path` (case-insensitive, like the server). */
export function ruleForPath<T extends AccessRuleLike>(
  rules: readonly T[],
  path: string,
): T | undefined {
  const target = normalizePath(path);
  let best: T | undefined;
  for (const rule of rules) {
    const rulePath = normalizePath(rule.path);
    const covers =
      rulePath === "" ||
      target === rulePath ||
      target.startsWith(`${rulePath}/`);
    if (
      covers &&
      (!best || rulePath.length > normalizePath(best.path).length)
    ) {
      best = rule;
    }
  }
  return best;
}

/** True when opening `entryPath` from `currentPath` enters a new protected area. */
export function entersProtectedArea(
  rules: readonly AccessRuleLike[],
  currentPath: string,
  entryPath: string,
) {
  const entryRule = ruleForPath(rules, entryPath);
  return Boolean(entryRule) && entryRule !== ruleForPath(rules, currentPath);
}

/** True when a rule is set on exactly this directory. */
export function hasRuleAt(rules: readonly AccessRuleLike[], path: string) {
  const target = normalizePath(path);
  return rules.some((rule) => normalizePath(rule.path) === target);
}

export function accessRuleLabel(path: string) {
  const normalized = path.trim().replace(/^\/+|\/+$/g, "");
  return normalized ? `/${normalized}` : t("整个站点");
}
