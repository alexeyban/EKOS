// Hand-stub of the console API response shapes.
//
// Run `npm run gen:api` (with the FastAPI server running on :8000) to regenerate
// `schema.d.ts` from its OpenAPI document and replace these by-hand types — RFC 0129 §6.

export interface Health {
  status: string;
  service: string;
  version: string;
}

export interface ServerStatus {
  state: "starting" | "ready" | "failed";
  port: number;
  retries: number;
  detail: string;
}

export interface Workspace {
  id: string;
  name: string;
  path: string;
  server: ServerStatus | null;
}

export interface StorageComponent {
  name: string;
  bytes: number;
  files: number;
}

export interface Stats {
  schema_version: number;
  workspace: string;
  backend: string;
  entries: number;
  objects: number;
  relationships: number;
  evidence: number | null;
  integrity: string;
  last_write: string | null;
  storage: { total_bytes: number; components: StorageComponent[] };
}

export interface DoctorCheck {
  name: string;
  status: "ok" | "warn" | "fail";
  detail: string;
}

export interface Doctor {
  schema_version: number;
  ok: boolean;
  checks: DoctorCheck[];
}

export interface KindCount {
  kind: string;
  count: number;
}

export interface TimelinePoint {
  t: string;
  objects: number;
  relationships: number;
}

export interface Timeline {
  schema_version: number;
  bucket: "day" | "week" | "month";
  points: TimelinePoint[];
}

export interface QueryStats {
  total: number;
  by_tool: Record<string, number>;
  cache_hit_rate: number;
  p50_ms: number;
  p95_ms: number;
}
