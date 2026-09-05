import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
import "@testing-library/jest-dom/vitest";

// @testing-library/react's own auto-cleanup only registers itself when `afterEach` is a global
// (i.e. `test.globals: true`) — this project imports it explicitly per file instead, so without
// this every test after the first in a file sees the previous test's DOM still mounted.
afterEach(() => {
  cleanup();
});
