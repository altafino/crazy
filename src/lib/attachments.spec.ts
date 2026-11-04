import { describe, expect, it } from "vitest";
import { middleTruncate, runBlockReason } from "./attachments";
import type { Chip } from "./attachments";

const chip = (state: Chip["state"]): Chip => ({ key: state, name: "x", kind: "text", state, visible: true });

describe("middleTruncate", () => {
  it("leaves 24 characters or fewer alone and keeps the extension of longer names", () => {
    expect(middleTruncate("a".repeat(24))).toBe("a".repeat(24));
    const t = middleTruncate("quarterly-report-final-version-2.pdf");
    expect(t).toHaveLength(24);
    expect(t.endsWith("n-2.pdf")).toBe(true);
    expect(t.startsWith("quarterly-rep")).toBe(true);
  });
});

describe("runBlockReason", () => {
  it("is null when every chip is ready and lets an error win over waiting", () => {
    expect(runBlockReason([chip("ready")])).toBeNull();
    expect(runBlockReason([chip("converting"), chip("adding")])).toBe("Waiting for 2 files to finish converting");
    expect(runBlockReason([chip("converting"), chip("error")])).toBe("Remove 1 file that can't be attached");
  });
});
