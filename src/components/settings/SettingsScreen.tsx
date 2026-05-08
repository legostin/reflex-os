import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  BRIDGE_API_GROUPS,
  BRIDGE_HELPER_GROUPS,
  BRIDGE_RECIPE_CARDS,
} from "../../appBridgeCatalog";
import {
  bridgeCatalogTitle,
  bridgeRecipeBody,
  bridgeRecipeTitle,
} from "../../appBridgeCatalogI18n";
import { useI18n, type LanguageSetting } from "../../i18n";

type LogLevel = "trace" | "debug" | "info" | "warn" | "error";
type RequestProfileId = "complex" | "fast" | "instant";

interface LogEntry {
  seq: number;
  ts_ms: number;
  level: LogLevel;
  source: string;
  message: string;
}

const LEVEL_ORDER: Record<LogLevel, number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
};

interface CodexReasoningLevel {
  effort: string;
  label: string;
  description: string;
}

interface CodexModel {
  slug: string;
  display_name: string;
  default_reasoning_level?: string | null;
  supported_reasoning_levels: CodexReasoningLevel[];
}

interface CodexSkill {
  name: string;
  display_name: string;
  description: string;
  source: string;
  path: string;
}

interface RequestProfile {
  model?: string | null;
  reasoning_effort?: string | null;
}

interface SystemSettings {
  version: number;
  request_profiles: Record<RequestProfileId, RequestProfile>;
  disabled_skills: string[];
}

interface SystemSettingsPayload {
  settings: SystemSettings;
  models: CodexModel[];
  skills: CodexSkill[];
  codex_home: string;
  settings_path: string;
  codex_model?: string | null;
  codex_reasoning_effort?: string | null;
}

type Tab = "agent" | "capabilities" | "logs";

const REQUEST_PROFILE_IDS: RequestProfileId[] = [
  "complex",
  "fast",
  "instant",
];

const FALLBACK_REASONING_LEVELS: CodexReasoningLevel[] = [
  {
    effort: "minimal",
    label: "minimal",
    description: "Smallest reasoning budget",
  },
  { effort: "low", label: "low", description: "Light reasoning" },
  { effort: "medium", label: "medium", description: "Balanced reasoning" },
  { effort: "high", label: "high", description: "Deep reasoning" },
  { effort: "xhigh", label: "xhigh", description: "Maximum reasoning" },
];

const CAPABILITY_GROUPS = [
  {
    titleKey: "cap.projects.title",
    bodyKey: "cap.projects.body",
  },
  {
    titleKey: "cap.topics.title",
    bodyKey: "cap.topics.body",
  },
  {
    titleKey: "cap.apps.title",
    bodyKey: "cap.apps.body",
  },
  {
    titleKey: "cap.memory.title",
    bodyKey: "cap.memory.body",
  },
  {
    titleKey: "cap.automations.title",
    bodyKey: "cap.automations.body",
  },
  {
    titleKey: "cap.mcp.title",
    bodyKey: "cap.mcp.body",
  },
] as const;

const PERMISSION_EXAMPLES = [
  "agent.project:<project>",
  "agent.project:*",
  "agent.cwd:*",
  "memory.global.read",
  "memory.global.write",
  "memory.project:*",
  "projects.read:*",
  "projects.write:<project>",
  "projects.write:*",
  "topics.read:<project>",
  "topics.read:*",
  "skills.read:<project>",
  "skills.read:*",
  "skills.write:<project>",
  "skills.write:*",
  "mcp.read:<project>",
  "mcp.read:*",
  "mcp.write:<project>",
  "mcp.write:*",
  "project.files.read:<project>",
  "project.files.read:*",
  "project.files.write:<project>",
  "project.files.write:*",
  "browser.read",
  "browser.control",
  "browser.project:<project>",
  "apps.create",
  "apps.manage",
  "apps.invoke:*",
  "apps.invoke:<app>",
  "scheduler.read:*",
  "scheduler.run:<app>",
  "scheduler.write:<app>::<schedule>",
  "net.fetch requires manifest.network.allowed_hosts",
] as const;

const BRIDGE_API_COUNT = BRIDGE_API_GROUPS.reduce(
  (sum, group) => sum + group.methods.length,
  0,
);

const BRIDGE_HELPER_COUNT = BRIDGE_HELPER_GROUPS.reduce(
  (sum, group) => sum + group.helpers.length,
  0,
);

const PERMISSION_COUNT = PERMISSION_EXAMPLES.length;

const SYSTEM_STATS = [
  {
    labelKey: "stats.bridgeMethods",
    value: BRIDGE_API_COUNT,
    detailKey: "stats.dispatchApi",
  },
  {
    labelKey: "stats.overlayHelpers",
    value: BRIDGE_HELPER_COUNT,
    detailKey: "stats.windowReflex",
  },
  {
    labelKey: "stats.workflows",
    value: BRIDGE_RECIPE_CARDS.length,
    detailKey: "stats.bridgeWorkflows",
  },
  {
    labelKey: "stats.permissionForms",
    value: PERMISSION_COUNT,
    detailKey: "stats.manifestGrants",
  },
] as const;

export function SettingsScreen() {
  const [tab, setTab] = useState<Tab>("agent");
  const { language, setLanguage, t } = useI18n();
  return (
    <div className="settings-root">
      <header className="settings-header">
        <h1>{t("settings.title")}</h1>
        <div className="settings-header-actions">
          <label className="settings-language-control">
            <span>{t("settings.languageLabel")}</span>
            <select
              value={language}
              onChange={(e) =>
                setLanguage(e.currentTarget.value as LanguageSetting)
              }
            >
              <option value="auto">{t("language.auto")}</option>
              <option value="en">{t("language.en")}</option>
              <option value="ru">{t("language.ru")}</option>
            </select>
          </label>
          <div className="settings-tabs">
            <button
              className={tab === "agent" ? "tab-on" : ""}
              onClick={() => setTab("agent")}
            >
              {t("settings.agentRouting")}
            </button>
            <button
              className={tab === "capabilities" ? "tab-on" : ""}
              onClick={() => setTab("capabilities")}
            >
              {t("settings.capabilities")}
            </button>
            <button
              className={tab === "logs" ? "tab-on" : ""}
              onClick={() => setTab("logs")}
            >
              {t("settings.logs")}
            </button>
          </div>
        </div>
      </header>
      {tab === "agent" ? (
        <AgentRoutingPane />
      ) : tab === "capabilities" ? (
        <CapabilitiesPane />
      ) : (
        <LogsPane />
      )}
    </div>
  );
}

function AgentRoutingPane() {
  const { t } = useI18n();
  const [payload, setPayload] = useState<SystemSettingsPayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [skillQuery, setSkillQuery] = useState("");

  useEffect(() => {
    let alive = true;
    setLoading(true);
    invoke<SystemSettingsPayload>("system_settings_get")
      .then((next) => {
        if (!alive) return;
        setPayload(next);
        setError(null);
      })
      .catch((e) => {
        if (!alive) return;
        setError(String(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  const disabledSet = useMemo(
    () => new Set(payload?.settings.disabled_skills ?? []),
    [payload?.settings.disabled_skills],
  );
  const normalizedSkillQuery = skillQuery.trim().toLowerCase();
  const visibleSkills = useMemo(() => {
    const skills = payload?.skills ?? [];
    if (!normalizedSkillQuery) return skills;
    return skills.filter((skill) =>
      [skill.name, skill.description, skill.source]
        .join(" ")
        .toLowerCase()
        .includes(normalizedSkillQuery),
    );
  }, [payload?.skills, normalizedSkillQuery]);

  function mutateSettings(update: (settings: SystemSettings) => void) {
    setPayload((prev) => {
      if (!prev) return prev;
      const next: SystemSettings = {
        ...prev.settings,
        request_profiles: {
          complex: { ...prev.settings.request_profiles.complex },
          fast: { ...prev.settings.request_profiles.fast },
          instant: { ...prev.settings.request_profiles.instant },
        },
        disabled_skills: [...prev.settings.disabled_skills],
      };
      update(next);
      return { ...prev, settings: next };
    });
    setDirty(true);
    setSaved(false);
  }

  function setProfileModel(profileId: RequestProfileId, model: string) {
    mutateSettings((settings) => {
      const profile = settings.request_profiles[profileId];
      profile.model = model;
      const levels = reasoningLevelsForModel(payload?.models ?? [], model);
      const validEfforts = new Set(levels.map((level) => level.effort));
      if (!profile.reasoning_effort || !validEfforts.has(profile.reasoning_effort)) {
        profile.reasoning_effort =
          levels[0]?.effort ?? profile.reasoning_effort ?? "medium";
      }
    });
  }

  function setProfileReasoning(profileId: RequestProfileId, effort: string) {
    mutateSettings((settings) => {
      settings.request_profiles[profileId].reasoning_effort = effort;
    });
  }

  function toggleSkill(skillName: string) {
    mutateSettings((settings) => {
      const next = new Set(settings.disabled_skills);
      if (next.has(skillName)) {
        next.delete(skillName);
      } else {
        next.add(skillName);
      }
      settings.disabled_skills = Array.from(next).sort((a, b) =>
        a.localeCompare(b),
      );
    });
  }

  async function saveSettings() {
    if (!payload) return;
    setSaving(true);
    setError(null);
    try {
      const next = await invoke<SystemSettingsPayload>("system_settings_save", {
        settings: payload.settings,
      });
      setPayload(next);
      setDirty(false);
      setSaved(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return (
      <div className="settings-pane">
        <div className="rounded-md border border-white/10 bg-white/[0.035] p-5 text-sm text-white/62">
          {t("settings.agentLoading")}
        </div>
      </div>
    );
  }

  if (!payload) {
    return (
      <div className="settings-pane">
        <div className="rounded-md border border-red-400/25 bg-red-500/10 p-5 text-sm text-red-100">
          {error ?? t("settings.agentLoadFailed")}
        </div>
      </div>
    );
  }

  return (
    <div className="settings-pane space-y-4">
      <section className="rounded-md border border-white/10 bg-white/[0.035] p-5 shadow-sm">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <h2 className="m-0 text-lg font-semibold text-white">
              {t("settings.agentTitle")}
            </h2>
            <p className="mt-2 max-w-3xl text-sm leading-6 text-white/62">
              {t("settings.agentBody")}
            </p>
          </div>
          <button
            type="button"
            className="rounded-md bg-sky-500/80 px-4 py-2 text-sm font-medium text-white transition hover:bg-sky-400/90 disabled:cursor-not-allowed disabled:bg-white/8 disabled:text-white/35"
            disabled={!dirty || saving}
            onClick={() => void saveSettings()}
          >
            {saving
              ? t("settings.agentSaving")
              : saved
                ? t("settings.agentSaved")
                : t("settings.agentSave")}
          </button>
        </div>
        {error ? (
          <div className="mt-4 rounded-md border border-red-400/25 bg-red-500/10 px-3 py-2 text-sm text-red-100">
            {error}
          </div>
        ) : null}
        <div className="mt-4 grid gap-2 text-xs text-white/45 md:grid-cols-2">
          <div>
            <span className="font-medium text-white/68">
              {t("settings.agentCodexHome")}:
            </span>{" "}
            <code>{payload.codex_home}</code>
          </div>
          <div>
            <span className="font-medium text-white/68">
              {t("settings.agentSettingsFile")}:
            </span>{" "}
            <code>{payload.settings_path}</code>
          </div>
        </div>
      </section>

      <section className="grid gap-3 xl:grid-cols-3">
        {REQUEST_PROFILE_IDS.map((profileId) => {
          const profile = payload.settings.request_profiles[profileId];
          const model = profile.model ?? payload.codex_model ?? "";
          const models = modelOptions(payload.models, model);
          const reasoningLevels = reasoningLevelsForModel(models, model);
          return (
            <article
              className="rounded-md border border-white/10 bg-white/[0.035] p-4 shadow-sm"
              key={profileId}
            >
              <div className="mb-4">
                <h3 className="m-0 text-base font-semibold text-white">
                  {t(`settings.profile.${profileId}`)}
                </h3>
                <p className="mt-1 text-sm leading-5 text-white/60">
                  {t(`settings.profile.${profileId}.body`)}
                </p>
              </div>
              <label className="mb-3 block text-xs font-semibold uppercase tracking-wide text-white/45">
                {t("settings.agentModel")}
                <select
                  className="mt-1 block w-full rounded-md border border-white/10 bg-black/25 px-3 py-2 text-sm font-medium normal-case tracking-normal text-white outline-none focus:border-sky-400/55 focus:ring-2 focus:ring-sky-400/15"
                  value={model}
                  onChange={(e) => setProfileModel(profileId, e.currentTarget.value)}
                >
                  {models.map((model) => (
                    <option key={model.slug} value={model.slug}>
                      {model.display_name} ({model.slug})
                    </option>
                  ))}
                </select>
              </label>
              <label className="block text-xs font-semibold uppercase tracking-wide text-white/45">
                {t("settings.agentReasoning")}
                <select
                  className="mt-1 block w-full rounded-md border border-white/10 bg-black/25 px-3 py-2 text-sm font-medium normal-case tracking-normal text-white outline-none focus:border-sky-400/55 focus:ring-2 focus:ring-sky-400/15"
                  value={profile.reasoning_effort ?? reasoningLevels[0]?.effort ?? "medium"}
                  onChange={(e) =>
                    setProfileReasoning(profileId, e.currentTarget.value)
                  }
                >
                  {reasoningLevels.map((level) => (
                    <option key={level.effort} value={level.effort}>
                      {level.label || level.effort}
                    </option>
                  ))}
                </select>
              </label>
              <p className="mt-3 text-xs leading-5 text-white/45">
                {reasoningLevels.find(
                  (level) => level.effort === profile.reasoning_effort,
                )?.description ?? t("settings.agentReasoningHint")}
              </p>
            </article>
          );
        })}
      </section>

      <section className="rounded-md border border-white/10 bg-white/[0.035] p-5 shadow-sm">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="m-0 text-lg font-semibold text-white">
              {t("settings.skillsTitle")}
            </h2>
            <p className="mt-1 text-sm text-white/60">
              {t("settings.skillsBody")}
            </p>
          </div>
          <span className="rounded-full bg-white/[0.06] px-3 py-1 text-xs font-semibold text-white/60 ring-1 ring-white/10">
            {t("settings.skillsDisabledCount", {
              count: payload.settings.disabled_skills.length,
              total: payload.skills.length,
            })}
          </span>
        </div>
        <input
          className="mt-4 w-full rounded-md border border-white/10 bg-black/25 px-3 py-2 text-sm text-white outline-none placeholder:text-white/32 focus:border-sky-400/55 focus:ring-2 focus:ring-sky-400/15"
          placeholder={t("settings.skillsSearch")}
          value={skillQuery}
          onChange={(e) => setSkillQuery(e.currentTarget.value)}
        />
        <div className="mt-4 grid max-h-[520px] gap-2 overflow-auto pr-1">
          {visibleSkills.length === 0 ? (
            <div className="rounded-md border border-dashed border-white/10 p-5 text-sm text-white/45">
              {t("settings.noMatches")}
            </div>
          ) : (
            visibleSkills.map((skill) => {
              const disabled = disabledSet.has(skill.name);
              return (
                <article
                  className={`rounded-md border p-3 transition ${
                    disabled
                      ? "border-rose-400/25 bg-rose-500/10"
                      : "border-white/10 bg-white/[0.025]"
                  }`}
                  key={skill.name}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <strong className="text-sm font-semibold text-white">
                          {skill.display_name}
                        </strong>
                        <span className="rounded-full bg-white/[0.05] px-2 py-0.5 text-[11px] font-medium text-white/45 ring-1 ring-white/10">
                          {skill.source}
                        </span>
                      </div>
                      {skill.description ? (
                        <p className="mt-1 text-sm leading-5 text-white/58">
                          {skill.description}
                        </p>
                      ) : null}
                    </div>
                    <button
                      type="button"
                      className={`shrink-0 rounded-md px-3 py-1.5 text-xs font-semibold ${
                        disabled
                          ? "bg-rose-500/85 text-white"
                          : "bg-white/[0.06] text-white/68 ring-1 ring-white/12 hover:bg-white/[0.09] hover:text-white"
                      }`}
                      onClick={() => toggleSkill(skill.name)}
                    >
                      {disabled
                        ? t("settings.skillDisabled")
                        : t("settings.skillEnabled")}
                    </button>
                  </div>
                </article>
              );
            })
          )}
        </div>
      </section>
    </div>
  );
}

function reasoningLevelsForModel(
  models: CodexModel[],
  modelSlug: string,
): CodexReasoningLevel[] {
  const model = models.find((candidate) => candidate.slug === modelSlug);
  if (model?.supported_reasoning_levels.length) {
    return model.supported_reasoning_levels;
  }
  return FALLBACK_REASONING_LEVELS;
}

function modelOptions(models: CodexModel[], selectedModel: string): CodexModel[] {
  if (models.length > 0) return models;
  if (!selectedModel) return [];
  return [
    {
      slug: selectedModel,
      display_name: selectedModel,
      default_reasoning_level: null,
      supported_reasoning_levels: FALLBACK_REASONING_LEVELS,
    },
  ];
}

function CapabilitiesPane() {
  const { t } = useI18n();
  const [bridgeQuery, setBridgeQuery] = useState("");
  const [copiedToken, setCopiedToken] = useState<string | null>(null);
  const normalizedBridgeQuery = bridgeQuery.trim().toLowerCase();

  const visibleApiGroups = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_API_GROUPS;
    return BRIDGE_API_GROUPS.map((group) => ({
      ...group,
      methods: group.methods.filter((method) =>
        method.toLowerCase().includes(normalizedBridgeQuery),
      ),
    })).filter((group) => group.methods.length > 0);
  }, [normalizedBridgeQuery]);

  const visibleHelperGroups = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_HELPER_GROUPS;
    return BRIDGE_HELPER_GROUPS.map((group) => ({
      ...group,
      helpers: group.helpers.filter((helper) =>
        helper.toLowerCase().includes(normalizedBridgeQuery),
      ),
    })).filter((group) => group.helpers.length > 0);
  }, [normalizedBridgeQuery]);

  const visibleRecipeCards = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_RECIPE_CARDS;
    return BRIDGE_RECIPE_CARDS.filter((recipe) =>
      [
        recipe.title,
        recipe.body,
        bridgeRecipeTitle(recipe, t),
        bridgeRecipeBody(recipe, t),
        recipe.example,
        ...recipe.calls,
      ]
        .join(" ")
        .toLowerCase()
        .includes(normalizedBridgeQuery),
    );
  }, [normalizedBridgeQuery, t]);

  const visiblePermissionExamples = useMemo(() => {
    if (!normalizedBridgeQuery) return PERMISSION_EXAMPLES;
    return PERMISSION_EXAMPLES.filter((permission) =>
      permission.toLowerCase().includes(normalizedBridgeQuery),
    );
  }, [normalizedBridgeQuery]);

  const visibleApiCount = visibleApiGroups.reduce(
    (sum, group) => sum + group.methods.length,
    0,
  );
  const visibleHelperCount = visibleHelperGroups.reduce(
    (sum, group) => sum + group.helpers.length,
    0,
  );

  async function copyToken(value: string) {
    try {
      await copyTextToClipboard(value);
      setCopiedToken(value);
      window.setTimeout(() => {
        setCopiedToken((current) => (current === value ? null : current));
      }, 1200);
    } catch (e) {
      console.warn("[reflex] settings copy failed", e);
    }
  }

  return (
    <div className="settings-pane capabilities-pane">
      <section className="settings-section">
        <h2>{t("settings.layerTitle")}</h2>
        <p>{t("settings.layerBody")}</p>
      </section>

      <div
        className="settings-stat-grid"
        aria-label={t("settings.summaryLabel")}
      >
        {SYSTEM_STATS.map((stat) => (
          <article className="settings-stat-card" key={stat.labelKey}>
            <span>{t(stat.labelKey)}</span>
            <strong>{stat.value}</strong>
            <small>{t(stat.detailKey)}</small>
          </article>
        ))}
      </div>

      <section className="settings-section settings-section-open">
        <h2>{t("settings.systemMap")}</h2>
        <div className="settings-cap-grid">
          {CAPABILITY_GROUPS.map((group) => (
            <article className="settings-cap-card" key={group.titleKey}>
              <h3>{t(group.titleKey)}</h3>
              <p>{t(group.bodyKey)}</p>
            </article>
          ))}
        </div>
      </section>

      <section className="settings-section settings-section-open">
        <div className="settings-section-title-row">
          <h2>{t("settings.bridgeTitle")}</h2>
          <div className="settings-section-controls">
            <input
              className="settings-bridge-search"
              placeholder={t("settings.bridgeSearch")}
              value={bridgeQuery}
              onChange={(e) => setBridgeQuery(e.currentTarget.value)}
            />
            <span className="settings-section-meta">
              {t("settings.methodsCount", {
                visible: visibleApiCount,
                total: BRIDGE_API_COUNT,
              })}
            </span>
          </div>
        </div>
        {visibleApiGroups.length === 0 ? (
          <div className="settings-empty-inline">
            {t("settings.noMatches")}
          </div>
        ) : (
          <div className="settings-api-grid">
            {visibleApiGroups.map((group) => (
              <article className="settings-api-group" key={group.title}>
                <h3>{bridgeCatalogTitle(group.title, t)}</h3>
                <div className="settings-method-list">
                  {group.methods.map((method) => (
                    <CopyToken
                      key={method}
                      copied={copiedToken === method}
                      value={method}
                      onCopy={copyToken}
                    />
                  ))}
                </div>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="settings-section settings-section-open">
        <div className="settings-section-title-row">
          <h2>{t("settings.recipesTitle")}</h2>
          <span className="settings-section-meta">
            {t("settings.recipesCount", {
              visible: visibleRecipeCards.length,
              total: BRIDGE_RECIPE_CARDS.length,
            })}
          </span>
        </div>
        {visibleRecipeCards.length === 0 ? (
          <div className="settings-empty-inline">
            {t("settings.noMatches")}
          </div>
        ) : (
          <div className="settings-recipe-grid">
            {visibleRecipeCards.map((recipe) => (
              <article className="settings-recipe-card" key={recipe.title}>
                <h3>{bridgeRecipeTitle(recipe, t)}</h3>
                <p>{bridgeRecipeBody(recipe, t)}</p>
                <div className="settings-method-list">
                  {recipe.calls.map((call) => (
                    <CopyToken
                      key={call}
                      copied={copiedToken === call}
                      value={call}
                      onCopy={copyToken}
                    />
                  ))}
                </div>
                <CopyToken
                  copied={copiedToken === recipe.example}
                  value={recipe.example}
                  onCopy={copyToken}
                  variant="example"
                />
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="settings-section settings-section-open">
        <div className="settings-section-title-row">
          <h2>{t("settings.helpersTitle")}</h2>
          <span className="settings-section-meta">
            {t("settings.helpersCount", {
              visible: visibleHelperCount,
              total: BRIDGE_HELPER_COUNT,
            })}
          </span>
        </div>
        {visibleHelperGroups.length === 0 ? (
          <div className="settings-empty-inline">
            {t("settings.noMatches")}
          </div>
        ) : (
          <div className="settings-helper-grid">
            {visibleHelperGroups.map((group) => (
              <article className="settings-api-group" key={group.title}>
                <h3>{bridgeCatalogTitle(group.title, t)}</h3>
                <div className="settings-method-list">
                  {group.helpers.map((helper) => (
                    <CopyToken
                      key={helper}
                      copied={copiedToken === helper}
                      value={helper}
                      onCopy={copyToken}
                    />
                  ))}
                </div>
              </article>
            ))}
          </div>
        )}
        <p className="settings-hint">{t("settings.helpersHint")}</p>
      </section>

      <section className="settings-section">
        <div className="settings-section-title-row">
          <h2>{t("settings.permissionsTitle")}</h2>
          <span className="settings-section-meta">
            {t("settings.grantsCount", {
              visible: visiblePermissionExamples.length,
              total: PERMISSION_COUNT,
            })}
          </span>
        </div>
        {visiblePermissionExamples.length === 0 ? (
          <div className="settings-empty-inline">
            {t("settings.noMatches")}
          </div>
        ) : (
          <div className="settings-token-list">
            {visiblePermissionExamples.map((permission) => (
              <CopyToken
                key={permission}
                copied={copiedToken === permission}
                value={permission}
                onCopy={copyToken}
                variant="permission"
              />
            ))}
          </div>
        )}
      </section>

      <section className="settings-section">
        <h2>{t("settings.automationFlow")}</h2>
        <div className="settings-flow">
          <span>{t("settings.flowSchedules")}</span>
          <span>{t("settings.flowRunner")}</span>
          <span>{t("settings.flowBridge")}</span>
          <span>{t("settings.flowHistory")}</span>
        </div>
        <p className="settings-hint">{t("settings.automationHint")}</p>
      </section>
    </div>
  );
}

function CopyToken({
  value,
  copied,
  onCopy,
  variant = "token",
}: {
  value: string;
  copied: boolean;
  onCopy: (value: string) => void | Promise<void>;
  variant?: "token" | "permission" | "example";
}) {
  const { t } = useI18n();
  return (
    <button
      className={`settings-copy-token settings-copy-${variant} ${copied ? "copied" : ""}`}
      onClick={() => void onCopy(value)}
      title={copied ? t("settings.copied") : t("settings.copy")}
      type="button"
    >
      {value}
    </button>
  );
}

async function copyTextToClipboard(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const ok = document.execCommand("copy");
  document.body.removeChild(textarea);
  if (!ok) throw new Error("Copy failed");
}

function LogsPane() {
  const { t } = useI18n();
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [paused, setPaused] = useState(false);
  const [filterText, setFilterText] = useState("");
  const [minLevel, setMinLevel] = useState<LogLevel>("info");
  const [filterSource, setFilterSource] = useState<string>("all");
  const lastSeqRef = useRef<number>(0);
  const pausedRef = useRef(false);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  useEffect(() => {
    let alive = true;
    invoke<LogEntry[]>("logs_get", { limit: 500 })
      .then((arr) => {
        if (!alive) return;
        setEntries(arr);
        if (arr.length > 0) lastSeqRef.current = arr[arr.length - 1].seq;
      })
      .catch(() => {});
    const u = listen<LogEntry>("reflex://logs/append", (ev) => {
      if (!ev.payload) return;
      lastSeqRef.current = ev.payload.seq;
      if (pausedRef.current) return;
      setEntries((prev) => {
        const next = [...prev, ev.payload];
        if (next.length > 2000) next.splice(0, next.length - 2000);
        return next;
      });
    });
    return () => {
      alive = false;
      u.then((un) => un());
    };
  }, []);

  useEffect(() => {
    if (paused) return;
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [entries, paused]);

  const sources = useMemo(() => {
    const set = new Set<string>();
    entries.forEach((e) => set.add(e.source));
    return Array.from(set).sort();
  }, [entries]);

  const visible = useMemo(() => {
    const minOrder = LEVEL_ORDER[minLevel];
    const text = filterText.trim().toLowerCase();
    return entries.filter((e) => {
      if (LEVEL_ORDER[e.level] < minOrder) return false;
      if (filterSource !== "all" && e.source !== filterSource) return false;
      if (text && !e.message.toLowerCase().includes(text)) return false;
      return true;
    });
  }, [entries, filterText, minLevel, filterSource]);

  return (
    <div className="settings-pane logs-pane">
      <div className="logs-toolbar">
        <select
          value={minLevel}
          onChange={(e) => setMinLevel(e.currentTarget.value as LogLevel)}
        >
          <option value="trace">trace+</option>
          <option value="debug">debug+</option>
          <option value="info">info+</option>
          <option value="warn">warn+</option>
          <option value="error">error</option>
        </select>
        <select
          value={filterSource}
          onChange={(e) => setFilterSource(e.currentTarget.value)}
        >
          <option value="all">{t("settings.allSources")}</option>
          {sources.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <input
          className="logs-search"
          placeholder={t("settings.logSearch")}
          value={filterText}
          onChange={(e) => setFilterText(e.currentTarget.value)}
        />
        <button onClick={() => setPaused((p) => !p)}>
          {paused ? `▶ ${t("settings.resume")}` : `⏸ ${t("settings.pause")}`}
        </button>
        <button
          onClick={() => setEntries([])}
          title={t("settings.clearTitle")}
        >
          {t("settings.clear")}
        </button>
        <span className="logs-count">
          {t("settings.rows", { count: visible.length })}
        </span>
      </div>
      <div className="logs-list" ref={listRef}>
        {visible.length === 0 ? (
          <div className="logs-empty">{t("settings.noLogs")}</div>
        ) : (
          visible.map((e) => (
            <div key={e.seq} className={`log-row log-${e.level}`}>
              <span className="log-time">
                {new Date(e.ts_ms).toLocaleTimeString()}
              </span>
              <span className={`log-level log-level-${e.level}`}>
                {e.level}
              </span>
              <span className="log-source">{e.source}</span>
              <span className="log-msg">{e.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

export default SettingsScreen;
