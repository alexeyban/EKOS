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

  let lastAnchor: HTMLAnchorElement | undefined;
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
    this: HTMLAnchorElement,
  ) {
    lastAnchor = this;
  });

  return {
    createObjectURL,
    revokeObjectURL,
    click,
    lastFilename: () => lastAnchor?.download,
    lastBlob: () => lastBlob,
  };
}
