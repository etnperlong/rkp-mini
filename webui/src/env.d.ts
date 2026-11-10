/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}

// Build-mode entry (vite.config.ts alias): sender or receiver main.
declare module '@app'
