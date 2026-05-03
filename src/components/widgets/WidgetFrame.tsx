import type { WidgetSource } from "./WidgetGrid";
import { useI18n } from "../../i18n";

const ALLOWED_SIZES = new Set(["small", "medium", "large", "wide"]);

interface Props {
  source: WidgetSource;
  onOpenApp?: (appId: string) => void;
}

export function WidgetFrame({ source, onOpenApp }: Props) {
  const { t } = useI18n();
  const { appId, appName, appIcon, widget } = source;
  const size = ALLOWED_SIZES.has(widget.size ?? "small")
    ? widget.size
    : "small";
  const entry = widget.entry.replace(/^\/+/, "");
  const src = `reflexapp://localhost/${encodeURIComponent(appId)}/${entry}`;

  return (
    <article className={`widget widget-${size}`}>
      <header className="widget-header">
        <button
          className="widget-source"
          onClick={() => onOpenApp?.(appId)}
          title={t("widget.openTitle", { name: appName })}
          disabled={!onOpenApp}
        >
          <span className="widget-source-icon">{appIcon ?? "🧩"}</span>
          <span className="widget-source-text">
            <span className="widget-name">{widget.name}</span>
            <span className="widget-app">{appName}</span>
          </span>
        </button>
      </header>
      <iframe
        className="widget-iframe"
        src={src}
        sandbox="allow-scripts allow-forms allow-same-origin"
        title={`${appName} — ${widget.name}`}
      />
    </article>
  );
}
