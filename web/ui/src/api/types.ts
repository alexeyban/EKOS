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

export interface ConfigOut {
  raw: string;
  observe: { paths: string[]; ignore_patterns: string[] };
}

export interface Finding {
  code: string;
  detail: string;
}

export interface ValidateResult {
  schema_version: number;
  ok: boolean;
  errors: Finding[];
  warnings: Finding[];
}

export interface ExtCount {
  ext: string;
  files: number;
  bytes: number;
}

export interface PreviewScan {
  schema_version: number;
  roots: string[];
  total_files: number;
  total_bytes: number;
  truncated: boolean;
  by_extension: ExtCount[];
  ignored_dir_hits: { pattern: string; dirs_skipped: number }[];
  elapsed_ms: number;
}

export interface WriteResult {
  written: boolean;
  observe_delta: {
    added_paths: string[];
    removed_paths: string[];
    added_patterns: string[];
    removed_patterns: string[];
  };
  warnings: Finding[];
  append_only_warning: string | null;
}

// RFC 0138 — ekos eval run/history. `file` is the saved report's own filename, the id
// GET /evals/reports/{file} takes.
export interface EvalReportSummary {
  file: string;
  dataset: string;
  agent: string;
  runtime: string;
  generated_at: string;
  status_pass: boolean;
  scenarios: number;
  passed: number;
  failed: number;
  answer_correctness: number | null;
  evidence_groundedness: number | null;
  completeness: number | null;
  recall_at_10: number | null;
  hallucination_rate: number;
  avg_tokens: number | null;
  p95_latency_ms: number;
  cache_hits: number;
  cache_misses: number;
  tokens_saved: number | null;
  peak_rss_kb: number | null;
  total_cpu_time_ms: number | null;
}

export interface EvalScenarioReport {
  id: string;
  passed: boolean;
  hallucinated: boolean;
  answer_score: number | null;
  evidence_score: number | null;
  completeness_score: number | null;
  retrieval_recall: number | null;
  groundedness_score: number | null;
  trajectory_score: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  cache_hit: boolean | null;
  rss_kb_end: number | null;
  cpu_time_ms: number | null;
  latency_ms: number;
  error: string | null;
}

export interface EvalGateThresholds {
  min_answer_correctness: number;
  min_evidence_groundedness: number;
  min_completeness: number;
  min_recall_at_10: number;
  max_hallucination_rate: number;
}

export interface EvalReport {
  dataset: string;
  agent: string;
  runtime: string;
  generated_at: string;
  gates: EvalGateThresholds;
  metrics: Omit<EvalReportSummary, "file">;
  scenarios: EvalScenarioReport[];
}
