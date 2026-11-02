import MarkdownIt from "markdown-it";

/**
 * Safe Markdown subset for agent text (D60). Raw HTML is escaped (`html: false`), images
 * become their alt text, headings become bold paragraphs, and only http(s) links stay links:
 * they render as `<a data-url>` plus the visible URL and are opened on click by the caller.
 */
const md = new MarkdownIt({ html: false, linkify: false, breaks: false });
const esc = md.utils.escapeHtml;
const isWeb = (href: string | null) => !!href && /^https?:\/\//i.test(href);

md.core.ruler.push("link-meta", (state) => {
  for (const block of state.tokens) {
    const stack: { meta: Record<string, unknown> | null }[] = [];
    for (const t of block.children ?? []) {
      if (t.type === "link_open") {
        t.meta = { href: t.attrGet("href") };
        stack.push(t);
      } else if (t.type === "link_close") t.meta = stack.pop()?.meta ?? null;
    }
  }
});
md.renderer.rules.link_open = (tokens, i) => {
  const href = (tokens[i]!.meta as { href: string | null }).href;
  return isWeb(href) ? `<a href="#" data-url="${esc(href!)}" class="md-link">` : "<span>";
};
md.renderer.rules.link_close = (tokens, i) => {
  const href = (tokens[i]!.meta as { href: string | null }).href;
  return isWeb(href) ? `</a> <span class="md-url">(${esc(href!)})</span>` : "</span>";
};
md.renderer.rules.image = (tokens, i) => esc(tokens[i]!.content);
md.renderer.rules.heading_open = () => "<p><strong>";
md.renderer.rules.heading_close = () => "</strong></p>\n";

export const renderMarkdown = (text: string): string => md.render(text);
