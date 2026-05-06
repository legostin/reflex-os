import { useI18n } from "../../i18n";
import { cx } from "../ui/primitives";
import type { Route } from "./workspaceTypes";

export const TAB_DRAG_TYPE = "application/reflex-tab";

export function PaneTabs({
  paneId,
  tabs,
  activeKey,
  canClosePane,
  getRouteKey,
  getRouteIcon,
  getRouteLabel,
  onActivate,
  onClose,
  onClosePane,
  onTabDragStart,
  onTabDragEnd,
}: {
  paneId: string;
  tabs: Route[];
  activeKey: string;
  canClosePane: boolean;
  getRouteKey: (route: Route) => string;
  getRouteIcon: (route: Route) => string;
  getRouteLabel: (route: Route) => string;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onClosePane: () => void;
  onTabDragStart: () => void;
  onTabDragEnd: () => void;
}) {
  const { t } = useI18n();

  return (
    <nav className="flex min-h-9 shrink-0 items-end gap-1 overflow-x-auto border-b border-white/10 bg-reflex-panel-2/70 px-2 pt-1">
      {tabs.map((route) => {
        const key = getRouteKey(route);
        const active = key === activeKey;
        const label = getRouteLabel(route);

        return (
          <div
            key={key}
            className={cx(
              "group inline-flex max-w-56 items-center gap-1 rounded-t-md border border-b-0 px-2.5 py-1.5 text-xs transition",
              active
                ? "border-white/14 bg-reflex-bg text-white"
                : "border-white/8 bg-white/[0.035] text-white/55 hover:bg-white/[0.06] hover:text-white/82",
            )}
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData(TAB_DRAG_TYPE, JSON.stringify({ paneId, key }));
              event.dataTransfer.effectAllowed = "move";
              onTabDragStart();
            }}
            onDragEnd={onTabDragEnd}
            onClick={() => onActivate(key)}
            onMouseDown={(event) => {
              if (event.button === 1) {
                event.preventDefault();
                onClose(key);
              }
            }}
            title={label}
          >
            <span className="shrink-0 text-white/55">{getRouteIcon(route)}</span>
            <span className="min-w-0 truncate">{label}</span>
            <button
              className="ml-1 inline-flex size-4 shrink-0 items-center justify-center rounded text-white/36 opacity-0 transition hover:bg-white/10 hover:text-white group-hover:opacity-100"
              onClick={(event) => {
                event.stopPropagation();
                onClose(key);
              }}
              title={t("nav.closeTab")}
              aria-label={t("nav.closeTab")}
            >
              ×
            </button>
          </div>
        );
      })}
      {canClosePane && (
        <button
          className="mb-1 inline-flex size-7 shrink-0 items-center justify-center rounded-md border border-white/10 bg-white/[0.035] text-xs text-white/45 transition hover:bg-white/[0.07] hover:text-white"
          onClick={onClosePane}
          title={t("nav.closePane")}
          aria-label={t("nav.closePane")}
        >
          ⨯
        </button>
      )}
    </nav>
  );
}
