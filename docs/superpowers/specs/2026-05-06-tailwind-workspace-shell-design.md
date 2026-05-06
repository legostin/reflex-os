# Tailwind Workspace Shell Design

## Goal

Reflex OS moves from a top-navigation interface to a workspace layout:

- Left sidebar: a persistent tree for system sections, project folders/projects, and utility folders/utilities.
- Right workspace: the active work area with panes/tabs and the existing project, topic, utility, memory, browser, automation, and settings screens.
- Tailwind CSS becomes the primary styling stack for the application UI.

This is a full UI migration, not only a shell wrapper. The end state should not rely on `src/components/ChatThread.css` as the main UI layer.

## Current State

- The main app renders `ChatThread` for the `main` Tauri window.
- `ChatThread.tsx` owns route state, project/thread loading, panes/tabs, top header navigation, and many screens.
- `ChatThread.css` is the main styling file and covers shell, tabs, project/app screens, topic UI, modals, drawers, settings, dashboard widgets, and responsive behavior.
- Projects and utilities already support folders at the backend level.
- Recent window fixes rely on `data-tauri-drag-region` plus Tauri window permissions.

## Information Architecture

The primary navigation tree is:

```text
Reflex
├─ Sections
│  ├─ Home
│  ├─ Memory
│  ├─ Automations
│  ├─ Browser
│  └─ Settings
├─ Projects
│  ├─ Folder
│  │  └─ Project
│  │     ├─ Topics
│  │     ├─ Files
│  │     └─ Utilities
└─ Utilities
   ├─ Folder
   │  └─ Utility
```

Project folders represent real folders on disk. Utility folders use the existing utility folder model.

## Layout

The main window uses a two-column desktop layout:

- `WorkspaceShell`: full-height app shell.
- `WorkspaceSidebar`: fixed-width left tree, roughly 260-300px on desktop.
- `WorkspaceMain`: right-side work area.
- `WorkspaceTitlebar`: compact drag region integrated at the top of the shell.

The old top navigation stops being the primary navigation. It is replaced by the sidebar tree. The right side may keep a compact context row for breadcrumbs, active route status, pane actions, and drag space.

On narrow viewports, the sidebar can collapse into an overlay or icon rail, but desktop macOS is the primary target.

## Routing And Workspace Behavior

Existing `Route`, `Layout`, panes, and tab behavior remain the routing foundation.

Sidebar clicks call the same route navigation path used by the current header:

- Section nodes open `home`, `memory`, `automations`, `browser`, or `settings`.
- Project nodes open `project:<id>`.
- Topic nodes open `topic:<id>`.
- Utility nodes open `app:<id>`.
- Utility folder and project folder nodes expand/collapse but do not need to open a separate screen in the first version.

Right-side panes/tabs remain available. Opening a node focuses an existing tab if it is already open, otherwise it opens in the focused pane.

## Data Loading

The shell needs a shared navigation data model:

- Projects from `list_projects`.
- Project folders from `list_project_folders`.
- Threads from `list_threads`.
- Utilities from `list_apps`.
- Utility folders from `list_app_folders`.

The data model should normalize these into tree nodes with stable IDs, labels, route targets, parent IDs, counts, and active state.

Backend APIs should not change unless the tree uncovers a missing field. The first implementation should use existing commands.

## Tailwind Migration

Tailwind is added as the primary styling system:

- Add Tailwind v4 and the official Vite plugin configuration.
- Replace global app styles with a Tailwind entry file.
- Use Tailwind utility classes and small shared React primitives instead of broad CSS selectors.
- Avoid a second design system. Shared primitives should be plain components such as `Button`, `IconButton`, `Panel`, `TreeItem`, `Badge`, `Input`, and `ModalFrame`.

Migration target:

- Shell/sidebar/titlebar: Tailwind only.
- Pane tabs and right workspace frame: Tailwind only.
- Home/project/apps/app viewer/topic/memory/browser/settings/automations screens: migrated to Tailwind classes or shared Tailwind primitives.
- Legacy CSS can remain only for narrow cases that are expensive or unsafe to replace in the first pass, such as markdown rendering, iframe boundaries, or temporary compatibility. Any remaining legacy CSS must be explicitly small and documented in the implementation summary.

## Visual Direction

The UI should feel like a desktop workbench, not a landing page:

- Dense but readable navigation.
- Calm dark macOS-style surface.
- Strong active/focused states.
- Compact rows, predictable hierarchy, and clear counts/status badges.
- No decorative gradient-orb backgrounds.
- Cards only for repeated items, modals, and framed tools; not for whole page sections.

## Window Dragging

The new titlebar and empty sidebar/header zones must preserve app dragging:

- Keep `data-tauri-drag-region` on non-interactive titlebar/sidebar header surfaces.
- Keep controls out of drag regions or mark them non-draggable through structure.
- Preserve `core:window:allow-start-dragging` and `core:window:allow-internal-toggle-maximize`.

## Testing And Verification

Required verification:

- `pnpm run build`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `git diff --check`
- Start `pnpm tauri dev`
- Verify `http://localhost:1420/` responds.
- Manually verify:
  - the sidebar tree renders sections, projects, project folders, utilities, and utility folders;
  - clicking nodes opens the right route;
  - panes/tabs still work;
  - window dragging works from the new titlebar/sidebar header area;
  - project and utility folder drag/drop behavior remains intact where it already existed.

## Implementation Boundaries

This migration may be large, but it should stay focused on UI structure and styling. It should not redesign backend storage, project creation semantics, utility runtime behavior, or app-server behavior unless a frontend integration bug requires a narrow fix.

The implementation should avoid unrelated refactors and should keep commits reviewable by grouping work around Tailwind setup, shell/sidebar, workspace frame, screen migration, and cleanup.
