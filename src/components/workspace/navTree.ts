import type { AppFolder, AppManifest, Project, ProjectFolder, Route, Thread } from "./workspaceTypes";

export type WorkspaceTreeNodeKind =
  | "group"
  | "section"
  | "project-folder"
  | "project"
  | "project-topics"
  | "project-files"
  | "project-utilities"
  | "topic"
  | "utility-folder"
  | "utility";

export type WorkspaceTreeNode = {
  id: string;
  kind: WorkspaceTreeNodeKind;
  label: string;
  icon?: string;
  route?: Route;
  children?: WorkspaceTreeNode[];
  count?: number;
};

type BuildWorkspaceTreeInput = {
  projects: Project[];
  projectFolders: ProjectFolder[];
  threads: Thread[];
  apps: AppManifest[];
  appFolders: AppFolder[];
};

const byName = <T extends { name: string }>(items: T[]) =>
  items.slice().sort((a, b) => a.name.localeCompare(b.name));

export function buildWorkspaceTree({
  projects,
  projectFolders,
  threads,
  apps,
  appFolders,
}: BuildWorkspaceTreeInput): WorkspaceTreeNode[] {
  const topicsByProject = new Map<string, Thread[]>();
  for (const thread of threads) {
    const list = topicsByProject.get(thread.project_id) ?? [];
    list.push(thread);
    topicsByProject.set(thread.project_id, list);
  }

  const appsByFolder = new Map<string, AppManifest[]>();
  for (const app of apps) {
    const folder = app.folder_path ?? "";
    const list = appsByFolder.get(folder) ?? [];
    list.push(app);
    appsByFolder.set(folder, list);
  }

  const toProjectNode = (project: Project): WorkspaceTreeNode => {
    const projectThreads = (topicsByProject.get(project.id) ?? []).slice().sort(
      (a, b) => b.created_at_ms - a.created_at_ms,
    );
    const linkedApps = apps.filter((app) => project.apps?.includes(app.id));

    return {
      id: `project:${project.id}`,
      kind: "project",
      label: project.name,
      icon: "project",
      route: { kind: "project", project_id: project.id },
      children: [
        {
          id: `project:${project.id}:topics`,
          kind: "project-topics",
          label: "Topics",
          icon: "topic",
          count: projectThreads.length,
          children: projectThreads.map((thread) => ({
            id: `topic:${thread.id}`,
            kind: "topic",
            label: (thread.title ?? thread.prompt.slice(0, 42)) || thread.id,
            icon: "topic",
            route: { kind: "topic", thread_id: thread.id },
          })),
        },
        {
          id: `project:${project.id}:files`,
          kind: "project-files",
          label: "Files",
          icon: "files",
          route: { kind: "project", project_id: project.id },
        },
        {
          id: `project:${project.id}:utilities`,
          kind: "project-utilities",
          label: "Utilities",
          icon: "utilities",
          count: linkedApps.length,
          route: { kind: "apps", project_id: project.id },
          children: linkedApps.map((app) => ({
            id: `project:${project.id}:utility:${app.id}`,
            kind: "utility",
            label: app.name,
            icon: app.icon ?? undefined,
            route: { kind: "app", app_id: app.id },
          })),
        },
      ],
    };
  };

  const projectsByFolder = new Map<string, Project[]>();
  for (const project of projects) {
    const folder = project.folder_path ?? "";
    const list = projectsByFolder.get(folder) ?? [];
    list.push(project);
    projectsByFolder.set(folder, list);
  }

  const projectFolderNodes: WorkspaceTreeNode[] = byName(projectFolders).map((folder) => ({
    id: `project-folder:${folder.path}`,
    kind: "project-folder" as const,
    label: folder.name,
    icon: "folder",
    count: folder.project_count,
    children: byName(projectsByFolder.get(folder.path) ?? []).map(toProjectNode),
  }));

  const rootProjectNodes = byName(projectsByFolder.get("") ?? []).map(toProjectNode);

  const utilityFolderNodes: WorkspaceTreeNode[] = byName(appFolders).map((folder) => ({
    id: `utility-folder:${folder.path}`,
    kind: "utility-folder" as const,
    label: folder.name,
    icon: "folder",
    children: byName(appsByFolder.get(folder.path) ?? []).map((app) => ({
      id: `utility:${app.id}`,
      kind: "utility" as const,
      label: app.name,
      icon: app.icon ?? undefined,
      route: { kind: "app" as const, app_id: app.id },
    })),
  }));

  const rootUtilities: WorkspaceTreeNode[] = byName(appsByFolder.get("") ?? []).map((app) => ({
    id: `utility:${app.id}`,
    kind: "utility" as const,
    label: app.name,
    icon: app.icon ?? undefined,
    route: { kind: "app" as const, app_id: app.id },
  }));

  const sectionNodes: WorkspaceTreeNode[] = [
    { id: "section:home", kind: "section", label: "Home", icon: "home", route: { kind: "home" } },
    { id: "section:memory", kind: "section", label: "Memory", icon: "memory", route: { kind: "memory" } },
    { id: "section:automations", kind: "section", label: "Automations", icon: "automation", route: { kind: "automations" } },
    { id: "section:browser", kind: "section", label: "Browser", icon: "browser", route: { kind: "browser" } },
    { id: "section:settings", kind: "section", label: "Settings", icon: "settings", route: { kind: "settings" } },
  ];

  return [
    ...sectionNodes,
    {
      id: "projects",
      kind: "group",
      label: "Projects",
      icon: "projects",
      count: projects.length,
      children: [...projectFolderNodes, ...rootProjectNodes],
    },
    {
      id: "utilities",
      kind: "group",
      label: "Utilities",
      icon: "utilities",
      count: apps.length,
      children: [...utilityFolderNodes, ...rootUtilities],
    },
  ];
}
