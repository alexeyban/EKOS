// Hand-stub of the console API response shapes.
//
// Run `npm run gen:api` (with the FastAPI server running on :8000) to regenerate
// `schema.d.ts` from its OpenAPI document and replace these by-hand types — RFC 0128 §3.4.

export interface Health {
  status: string;
  service: string;
  version: string;
}

export interface Workspace {
  id: string;
  name: string;
  path: string;
}

export interface Stats {
  entries: number;
  objects: number;
  relationships: number | null;
}
