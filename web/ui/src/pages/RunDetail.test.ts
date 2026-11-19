import { describe, expect, it } from "vitest";
import { runSummary } from "./RunDetail";

const stages = (states: string[]) => states.map((status, i) => ({ name: `s${i}`, status, exit_code: null }));

describe("run summary", () => {
  it("says which stage of how many is running", () => {
    expect(
      runSummary({ status: "running", stages: stages(["succeeded", "succeeded", "succeeded", "running", "pending", "pending", "pending"]) }),
    ).toBe("running · stage 4 of 7");
  });
  it("says all stages are done, or the plain status", () => {
    expect(runSummary({ status: "succeeded", stages: stages(["succeeded", "succeeded"]) })).toBe(
      "succeeded · all 2 stages done",
    );
    expect(runSummary({ status: "timed_out", stages: [] })).toBe("timed out");
  });
});
