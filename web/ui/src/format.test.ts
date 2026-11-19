import { describe, expect, it } from "vitest";
import { commandTitle, relativeTime } from "./format";

describe("format", () => {
  it("turns a command name into a title", () => {
    expect(commandTitle("init-and-compile")).toBe("Init and compile");
    expect(commandTitle("ledger_repair")).toBe("Ledger repair");
    expect(commandTitle("build")).toBe("Build");
  });

  it("says how long ago, reading a time without a zone as UTC", () => {
    const now = Date.parse("2026-11-18T12:00:00Z");
    expect(relativeTime("2026-11-18T11:48:00", now)).toBe("12 minutes ago");
    expect(relativeTime("2026-11-18T11:59:50Z", now)).toBe("just now");
    expect(relativeTime("2026-11-18T09:00:00+00:00", now)).toBe("3 hours ago");
    expect(relativeTime("2026-11-18T13:00:00+02:00", now)).toBe("1 hour ago");
    // A "z" that is not the zone at the end does not stop the UTC reading.
    expect(relativeTime("zzz", now)).toBe("zzz");
    expect(relativeTime("not a time", now)).toBe("not a time");
  });
});
