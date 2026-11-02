import { describe, expect, it } from "vitest";
import { binaryCopy, clockTime, formatSize } from "./format";

describe("format", () => {
  it("formats sizes", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(12 * 1024)).toBe("12 KB");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatSize(5 * 1024 * 1024)).toBe("5 MB");
  });
  it("words binary changes by which sides exist", () => {
    expect(binaryCopy(12288, 15360)).toBe("Binary file changed (12 KB → 15 KB)");
    expect(binaryCopy(null, 15360)).toBe("Binary file added (15 KB)");
    expect(binaryCopy(12288, null)).toBe("Binary file deleted (12 KB)");
    expect(binaryCopy(undefined, undefined)).toBe("Binary file changed");
  });
  it("shows local HH:MM:SS, nothing for unknown", () => {
    const ms = new Date(2026, 10, 2, 14, 30, 21).getTime();
    expect(clockTime(ms)).toBe("14:30:21");
    expect(clockTime(0)).toBe("");
    expect(clockTime(undefined)).toBe("");
  });
});
