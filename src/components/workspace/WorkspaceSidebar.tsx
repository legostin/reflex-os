import { useMemo, useState } from "react";
import { Badge, Button, cx } from "../ui/primitives";
import type { WorkspaceTreeNode } from "./navTree";
import type { Route } from "./workspaceTypes";

const ICONS: Record<string, string> = {
  sections: "☰",
  home: "⌂",
  memory: "◈",
  automation: "⏱",
  browser: "◎",
  settings: "⚙",
  projects: "▣",
  project: "▤",
  folder: "▸",
  topic: "●",
  files: "#",
  utilities: "◇",
  utility: "◆",
};

function iconForNode(node: WorkspaceTreeNode): string {
  if (node.icon) return ICONS[node.icon] ?? node.icon;
  return (
    {
      group: "☰",
      section: "·",
      "project-folder": "▸",
      project: "▤",
      "project-topics": "●",
      "project-files": "#",
      "project-utilities": "◇",
      topic: "•",
      "utility-folder": "▸",
      utility: "◆",
    } satisfies Record<WorkspaceTreeNode["kind"], string>
  )[node.kind];
}

function iconToneForNode(node: WorkspaceTreeNode): string {
  const key = node.icon ?? node.kind;
  if (key === "home") return "border-sky-400/20 bg-sky-400/12 text-sky-200";
  if (key === "memory") return "border-emerald-400/20 bg-emerald-400/12 text-emerald-200";
  if (key === "automation") return "border-amber-400/20 bg-amber-400/12 text-amber-200";
  if (key === "browser") return "border-cyan-400/20 bg-cyan-400/12 text-cyan-200";
  if (key === "settings") return "border-zinc-300/18 bg-zinc-300/10 text-zinc-200";
  if (key === "projects" || key === "project") return "border-blue-400/20 bg-blue-400/12 text-blue-200";
  if (key === "folder" || node.kind === "project-folder" || node.kind === "utility-folder") {
    return "border-yellow-300/20 bg-yellow-300/12 text-yellow-100";
  }
  if (key === "topic") return "border-violet-400/20 bg-violet-400/12 text-violet-200";
  if (key === "files") return "border-stone-300/18 bg-stone-300/10 text-stone-200";
  if (key === "utilities" || key === "utility") return "border-fuchsia-400/20 bg-fuchsia-400/12 text-fuchsia-200";
  return "border-white/10 bg-white/[0.045] text-white/62";
}

export function WorkspaceSidebar({
  tree,
  activeRouteKey,
  routeKey,
  onNavigate,
  onCreateProject,
}: {
  tree: WorkspaceTreeNode[];
  activeRouteKey: string;
  routeKey: (route: Route) => string;
  onNavigate: (route: Route) => void;
  onCreateProject: () => void;
}) {
  const initialExpanded = useMemo(() => new Set(["projects", "utilities"]), []);
  const [expanded, setExpanded] = useState(initialExpanded);

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="border-b border-white/10 px-3 pb-3" data-tauri-drag-region>
        <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-white/36">Reflex OS</div>
        <div className="mt-2 flex items-center gap-2">
          <Button className="flex-1" variant="primary" onClick={onCreateProject}>
            New project
          </Button>
        </div>
      </div>
      <nav className="min-h-0 flex-1 overflow-auto px-2.5 py-4">
        {tree.map((node) => (
          <TreeNode
            key={node.id}
            node={node}
            depth={0}
            expanded={expanded}
            activeRouteKey={activeRouteKey}
            routeKey={routeKey}
            onToggle={toggle}
            onNavigate={onNavigate}
          />
        ))}
      </nav>
    </div>
  );
}

function TreeNode({
  node,
  depth,
  expanded,
  activeRouteKey,
  routeKey,
  onToggle,
  onNavigate,
}: {
  node: WorkspaceTreeNode;
  depth: number;
  expanded: Set<string>;
  activeRouteKey: string;
  routeKey: (route: Route) => string;
  onToggle: (id: string) => void;
  onNavigate: (route: Route) => void;
}) {
  const hasChildren = !!node.children?.length;
  const open = expanded.has(node.id);
  const active = node.route ? routeKey(node.route) === activeRouteKey : false;
  const action = () => {
    if (hasChildren && !node.route) onToggle(node.id);
    else if (node.route) onNavigate(node.route);
    else onToggle(node.id);
  };

  return (
    <div className="py-0.5">
      <button
        className={cx(
          "flex min-h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[13px] font-medium leading-5 transition",
          active ? "bg-reflex-accent/18 text-white" : "text-white/68 hover:bg-white/[0.06] hover:text-white/90",
        )}
        style={{ paddingLeft: 8 + depth * 14 }}
        onClick={action}
      >
        <span className="w-3 text-[11px] text-white/34">{hasChildren ? (open ? "▾" : "▸") : ""}</span>
        <span
          className={cx(
            "inline-flex size-6 shrink-0 items-center justify-center rounded-md border text-[12px] font-semibold shadow-sm",
            active
              ? "border-reflex-accent/35 bg-reflex-accent/20 text-white"
              : iconToneForNode(node),
          )}
          aria-hidden="true"
        >
          {iconForNode(node)}
        </span>
        <span className="min-w-0 flex-1 truncate">{node.label}</span>
        {typeof node.count === "number" ? <Badge className="px-1.5 py-0 text-[11px]">{node.count}</Badge> : null}
      </button>
      {hasChildren && open ? (
        <div className="mt-1">
          {node.children!.map((child) => (
            <TreeNode
              key={child.id}
              node={child}
              depth={depth + 1}
              expanded={expanded}
              activeRouteKey={activeRouteKey}
              routeKey={routeKey}
              onToggle={onToggle}
              onNavigate={onNavigate}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}
