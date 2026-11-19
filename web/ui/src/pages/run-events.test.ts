import { describe, expect, it } from "vitest";
import { modelCalls, stepRows } from "./run-events";

const events = [
  { stage: "run", msg: "started", version: "1.1.0", cmd: "build", t_ms: 0 },
  { stage: "java.parse", msg: "parsed", files: 4589, ok: 4500, cmd: "recover", t_ms: 840 },
  { stage: "warning", msg: "scan failed", target: "ekos::x", cmd: "recover", t_ms: 900 },
  { stage: "llm", msg: "calls", count: 0, cache_hits: 0, embeddings: 0, cmd: "recover", t_ms: 901 },
  { stage: "llm", msg: "calls", count: 2, cache_hits: 0, embeddings: 0, cmd: "commit", t_ms: 5 },
  { stage: "run", msg: "finished", ok: false, ms: 6, cmd: "commit", t_ms: 6 },
];

describe("stepRows", () => {
  it("keeps the line's own keys apart and lists the counts in order", () => {
    const rows = stepRows(events);
    expect(rows[1]).toEqual({
      cmd: "recover",
      stage: "java.parse",
      msg: "parsed",
      counts: "files 4589 · ok 4500",
      tMs: 840,
      tone: "",
    });
  });

  it("marks warnings, failures and model calls", () => {
    const tones = stepRows(events).map((r) => r.tone);
    expect(tones).toEqual(["", "", "warn", "ok", "warn", "bad"]);
  });
});

describe("modelCalls", () => {
  it("sums every command's llm line, and is null before any", () => {
    expect(modelCalls(events)).toEqual({ completions: 2, embeddings: 0 });
    expect(modelCalls(events.slice(0, 2))).toBeNull();
  });
});

describe("stage times", () => {
  it("takes each command's largest t_ms", async () => {
    const { stageTimes, clock } = await import("./run-events");
    expect(
      stageTimes([
        { cmd: "build", t_ms: 100 },
        { cmd: "build", t_ms: 221_000 },
        { cmd: "recover", t_ms: 5 },
        { stage: "x" },
      ]),
    ).toEqual({ build: 221_000, recover: 5 });
    expect(clock(2_000)).toBe("0:02");
    expect(clock(221_000)).toBe("3:41");
    expect(clock(3_725_000)).toBe("1:02:05");
  });
});
