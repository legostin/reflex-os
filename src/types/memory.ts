export type MemoryScope = "global" | "project" | "topic";
export type MemoryKind =
  | "user"
  | "project"
  | "feedback"
  | "reference"
  | "tool"
  | "system"
  | "fact";

export interface NoteFrontmatter {
  id: string;
  name: string;
  description: string;
  type: MemoryKind;
  tags: string[];
  created_at_ms: number;
  updated_at_ms: number;
  source?: string | null;
}

export interface MemoryNote {
  scope: MemoryScope;
  path: string;
  rel_path: string;
  front: NoteFrontmatter;
  body: string;
}

export interface RagHit {
  doc_id: string;
  source?: string | null;
  chunk: string;
  score: number;
  kind: string;
}

export interface MemoryRef {
  scope: MemoryScope;
  rel_path: string;
}

export interface RecallResult {
  markdown: string;
  notes: MemoryRef[];
  rag: RagHit[];
}

export const MEMORY_SCOPES: MemoryScope[] = ["global", "project", "topic"];
export const MEMORY_KINDS: MemoryKind[] = [
  "user",
  "project",
  "feedback",
  "reference",
  "tool",
  "system",
  "fact",
];
