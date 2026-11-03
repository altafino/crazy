import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import UninstallDialog from "./UninstallDialog.vue";

describe("UninstallDialog", () => {
  it("names what goes and emits confirm/cancel", () => {
    const w = mount(UninstallDialog, { props: { appName: "Notes", version: "1.1.0", releasesKept: 1 }, attachTo: document.body });
    expect(document.body.textContent).toContain("Uninstall Notes?");
    expect(document.body.textContent).toContain("1 kept release file");
    const b = [...document.body.querySelectorAll("button")];
    b.find((x) => x.textContent === "Uninstall")!.click();
    b.find((x) => x.textContent === "Cancel")!.click();
    expect(w.emitted("confirm")).toHaveLength(1);
    expect(w.emitted("cancel")).toHaveLength(1);
    w.unmount();
  });
});
