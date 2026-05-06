import type { ReactNode } from "react";
import { Badge, Button } from "../ui/primitives";
import type { Route } from "./workspaceTypes";

export function WorkspaceMain({
  title,
  route,
  onAddPane,
  children,
}: {
  title: string;
  route: Route;
  onAddPane: () => void;
  children: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-12 shrink-0 items-center justify-between gap-3 border-b border-white/10 bg-reflex-bg px-4">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-white">{title}</div>
          <div className="mt-0.5 text-[11px] uppercase tracking-[0.08em] text-white/38">{route.kind}</div>
        </div>
        <div className="flex items-center gap-2">
          <Badge>Workspace</Badge>
          <Button variant="secondary" onClick={onAddPane}>
            New pane
          </Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
    </div>
  );
}
