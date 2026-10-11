import type {
  AuditFeature,
  AuditRule,
  AuditSettings,
  SaveAuditSettings,
} from "../../../api/admin/logs";

export type AuditDraft = {
  enabled: boolean;
  rules: Record<string, AuditRule>;
};

export type FeatureGroup = { group: string; features: AuditFeature[] };

export function draftFrom(settings: AuditSettings): AuditDraft {
  return {
    enabled: settings.enabled,
    rules: Object.fromEntries(
      settings.features.map((feature) => [
        feature.key,
        {
          enabled: feature.enabled,
          log_reads: feature.log_reads,
          log_body: feature.log_body,
        },
      ]),
    ),
  };
}

/** Groups features in their original order, optionally filtered. */
export function groupFeatures(
  features: AuditFeature[],
  keyword = "",
): FeatureGroup[] {
  const words = keyword.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const groups: FeatureGroup[] = [];
  for (const feature of features) {
    const haystack = [
      feature.label,
      feature.key,
      feature.group,
      ...feature.prefixes,
    ]
      .join(" ")
      .toLowerCase();
    if (!words.every((word) => haystack.includes(word))) continue;
    let group = groups.find((entry) => entry.group === feature.group);
    if (!group) {
      group = { group: feature.group, features: [] };
      groups.push(group);
    }
    group.features.push(feature);
  }
  return groups;
}

export function setRule(
  draft: AuditDraft,
  key: string,
  patch: Partial<AuditRule>,
): AuditDraft {
  return {
    ...draft,
    rules: { ...draft.rules, [key]: { ...draft.rules[key], ...patch } },
  };
}

/** Applies the same patch to the given features (all when omitted). */
export function setAll(
  draft: AuditDraft,
  patch: Partial<AuditRule>,
  keys: string[] = Object.keys(draft.rules),
): AuditDraft {
  const rules = { ...draft.rules };
  for (const key of keys) rules[key] = { ...rules[key], ...patch };
  return { ...draft, rules };
}

export function resetToDefaults(
  draft: AuditDraft,
  features: AuditFeature[],
): AuditDraft {
  return {
    enabled: true,
    rules: Object.fromEntries(
      features.map((feature) => [feature.key, { ...feature.defaults }]),
    ),
  };
}

export function toPayload(draft: AuditDraft): SaveAuditSettings {
  return {
    enabled: draft.enabled,
    features: Object.entries(draft.rules).map(([key, rule]) => ({
      key,
      ...rule,
    })),
  };
}

export function sameDraft(a: AuditDraft, b: AuditDraft) {
  if (a.enabled !== b.enabled) return false;
  const keys = new Set([...Object.keys(a.rules), ...Object.keys(b.rules)]);
  for (const key of keys) {
    const left = a.rules[key];
    const right = b.rules[key];
    if (
      !left ||
      !right ||
      left.enabled !== right.enabled ||
      left.log_reads !== right.log_reads ||
      left.log_body !== right.log_body
    ) {
      return false;
    }
  }
  return true;
}

/** How many features record anything, for the summary line. */
export function countRecording(draft: AuditDraft) {
  return Object.values(draft.rules).filter((rule) => rule.enabled).length;
}
