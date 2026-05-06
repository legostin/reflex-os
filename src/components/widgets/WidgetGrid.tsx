import { WidgetFrame } from "./WidgetFrame";
import { useI18n } from "../../i18n";

export interface WidgetDef {
  id: string;
  name: string;
  entry: string;
  size?: string;
  description?: string | null;
}

export interface WidgetSource {
  appId: string;
  appName: string;
  appIcon?: string | null;
  widget: WidgetDef;
}

interface Props {
  sources: WidgetSource[];
  onOpenApp?: (appId: string) => void;
}

export function WidgetGrid({ sources, onOpenApp }: Props) {
  const { t } = useI18n();
  if (sources.length === 0) {
    return (
      <div className="widget-grid-empty">
        {t("widget.empty")}
      </div>
    );
  }
  return (
    <div className="widget-grid">
      {sources.map((s) => (
        <WidgetFrame
          key={`${s.appId}::${s.widget.id}`}
          source={s}
          onOpenApp={onOpenApp}
        />
      ))}
    </div>
  );
}
