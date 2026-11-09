import { describe, expect, it } from "vitest";
import changelog from "../../CHANGELOG.md?raw";
import pkg from "../../package.json";
import { releaseNotes } from "./changelog";

const links = "\n[Unreleased]: https://x/compare\n[0.2.0]: https://x/2\n";
const doc = (unreleased: string) => `# Changelog\n\nPreamble.\n\n## [Unreleased]\n\n${unreleased}\n\n## [0.2.0] - 2026-11-09\n\nIntro two.\n\n### Added\n\n- **A.** thing\n\n### Changed\n\n- B\n\n## [0.1.0] - 2026-11-08\n\n### Added\n\n**Day (8 Nov)**\n- C\n${links}`;

const summary = (raw: string, v: string) => releaseNotes(raw, v).blocks.map((b) => [b.title, b.badge, b.open]);

describe("releaseNotes", () => {
  it.each([
    ["nothing yet is skipped, running version open", doc("Nothing yet."), "0.1.0", [["0.2.0", null, false], ["0.1.0", "this", true]]],
    ["unreleased with entries is first, open, badged", doc("### Added\n\n- Soon"), "0.2.0", [["Unreleased", "unreleased", true], ["0.2.0", "this", true], ["0.1.0", null, false]]],
    ["unreleased with only an intro counts as entries", doc("Some words."), "0.2.0", [["Unreleased", "unreleased", true], ["0.2.0", "this", true], ["0.1.0", null, false]]],
    ["running version missing: no badge, newest released open", doc("Nothing yet."), "9.9.9", [["0.2.0", null, true], ["0.1.0", null, false]]],
    ["empty input", "", "0.2.0", []],
    ["unparseable input", "just some text\n- a bullet", "0.2.0", []],
  ])("%s", (_name, raw, version, expected) => {
    expect(summary(raw as string, version as string)).toEqual(expected);
  });

  it("parses dates, intro, groups, bold day lines, and drops link references", () => {
    const [v2, v1] = releaseNotes(doc("Nothing yet."), "0.2.0").blocks;
    expect(v2).toMatchObject({ title: "0.2.0", dateText: "9 Nov 2026", intro: "Intro two." });
    expect(v2!.groups).toEqual([
      { heading: "Added", markdown: "- **A.** thing" },
      { heading: "Changed", markdown: "- B" },
    ]);
    expect(v1!.dateText).toBe("8 Nov 2026");
    expect(v1!.groups[0]!.markdown).toBe("**Day (8 Nov)**\n- C");
    expect(JSON.stringify(v1)).not.toContain("compare");
  });

  it("Unreleased has an empty date", () => {
    expect(releaseNotes(doc("### Added\n\n- x"), "0.2.0").blocks[0]!.dateText).toBe("");
  });

  it("the real CHANGELOG.md has a block for the version in package.json (eng D8)", () => {
    const blocks = releaseNotes(changelog, pkg.version).blocks;
    expect(blocks.find((b) => b.badge === "this")?.title).toBe(pkg.version);
  });
});
