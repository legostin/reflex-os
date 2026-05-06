import type { ReactNode } from "react";
import { cx } from "../ui/primitives";

export function WorkspaceShell({
  sidebar,
  titlebar,
  children,
}: {
  sidebar: ReactNode;
  titlebar: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-reflex-bg text-reflex-text">
      <aside className="flex w-[280px] shrink-0 flex-col border-r border-white/10 bg-reflex-panel/95">
        <div className="h-7 shrink-0" data-tauri-drag-region />
        {sidebar}
      </aside>
      <section className="flex min-w-0 flex-1 flex-col">
        <div className="min-h-10 shrink-0 border-b border-white/10 bg-reflex-bg/96" data-tauri-drag-region>
          <div className={cx("flex min-h-10 items-center px-4", "[&_*]:select-none")}>{titlebar}</div>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
      </section>
    </div>
  );
}
