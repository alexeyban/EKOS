import { vi } from "vitest";

/** Stubs the browser download machinery `graph-export.ts`'s `triggerDownload` drives, and
 * captures the last `<a download>` click so a test can assert on the filename/blob it produced. */
export function mockDownload() {
  let lastBlob: Blob | undefined;
  const createObjectURL = vi.fn((blob: Blob) => {
    lastBlob = blob;
    return "blob:mock-url";
  });
  const revokeObjectURL = vi.fn();
  vi.stubGlobal("URL", { ...URL, createObjectURL, revokeObjectURL });

  // The spy records each call's `this` in `mock.contexts`, so the clicked anchor is read from
  // there rather than captured by the implementation.
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
  const lastAnchor = () => click.mock.contexts.at(-1) as HTMLAnchorElement | undefined;

  return {
    createObjectURL,
    revokeObjectURL,
    click,
    lastFilename: () => lastAnchor()?.download,
    lastBlob: () => lastBlob,
  };
}
