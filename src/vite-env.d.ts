/// <reference types="vite/client" />

/** Build info injected by vite.config.ts (fixed values in vitest.config.ts), shown on About. */
declare const __APP_COMMIT__: string;
declare const __APP_BUILT__: string;

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
