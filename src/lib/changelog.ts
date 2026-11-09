/** Turns the bundled CHANGELOG.md (Keep a Changelog) into what the About page draws (D6, D7). */
export interface Group { heading: string; markdown: string }
export interface Block {
  title: string;
  dateText: string;
  badge: "this" | "unreleased" | null;
  open: boolean;
  intro: string;
  groups: Group[];
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const HEAD = /^## \[([^\]]+)\](?:\s*-\s*(\d{4})-(\d{2})-(\d{2}))?\s*$/;

interface Draft { title: string; dateText: string; intro: string[]; groups: { heading: string; lines: string[] }[] }

export function releaseNotes(raw: string, runningVersion: string): { blocks: Block[] } {
  const drafts: Draft[] = [];
  let cur: Draft | undefined;
  for (const line of String(raw ?? "").split(/\r?\n/)) {
    const h = HEAD.exec(line);
    if (h) {
      const [, title, y, m, d] = h;
      cur = { title: title!, dateText: y ? `${Number(d)} ${MONTHS[Number(m) - 1] ?? m} ${y}` : "", intro: [], groups: [] };
      drafts.push(cur);
      continue;
    }
    if (!cur || /^\[[^\]]+\]:\s/.test(line)) continue; // before the first version, or a link reference
    const g = /^### (.+?)\s*$/.exec(line);
    if (g) cur.groups.push({ heading: g[1]!, lines: [] });
    else (cur.groups[cur.groups.length - 1]?.lines ?? cur.intro).push(line);
  }

  const blocks: Block[] = drafts.map((d) => ({
    title: d.title,
    dateText: d.dateText,
    badge: null,
    open: false,
    intro: d.intro.join("\n").trim(),
    groups: d.groups.map((g) => ({ heading: g.heading, markdown: g.lines.join("\n").trim() })),
  }));

  const unreleased = blocks.filter((b) => b.title === "Unreleased" && (b.groups.length > 0 || !/^(nothing yet\.?)?$/i.test(b.intro)));
  const released = blocks.filter((b) => b.title !== "Unreleased");
  for (const b of unreleased) { b.badge = "unreleased"; b.open = true; }
  const running = released.find((b) => b.title === runningVersion);
  if (running) { running.badge = "this"; running.open = true; }
  else if (released[0]) released[0].open = true;
  return { blocks: [...unreleased, ...released] };
}
