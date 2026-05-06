import { useMemo, useState } from "react";
import { Badge, Button, cx } from "../ui/primitives";
import type { WorkspaceTreeNode } from "./navTree";
import type { Route } from "./workspaceTypes";

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
  const initialExpanded = useMemo(() => new Set(["sections", "projects", "utilities"]), []);
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
      <nav className="min-h-0 flex-1 overflow-auto px-2 py-3">
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
    <div>
      <button
        className={cx(
          "flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-xs transition",
          active ? "bg-reflex-accent/18 text-white" : "text-white/64 hover:bg-white/[0.06] hover:text-white/88",
        )}
        style={{ paddingLeft: 8 + depth * 14 }}
        onClick={action}
      >
        <span className="w-3 text-white/34">{hasChildren ? (open ? "▾" : "▸") : ""}</span>
        <span className="min-w-0 flex-1 truncate">{node.label}</span>
        {typeof node.count === "number" ? <Badge className="px-1.5 py-0 text-[10px]">{node.count}</Badge> : null}
      </button>
      {hasChildren && open ? (
        <div className="mt-0.5">
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
