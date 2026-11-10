/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}

// Build-mode entry (vite.config.ts alias): the selected app's main.
declare module '@app'

// Build-mode configuration (vite.config.ts alias): apps/<app>/config.ts.
declare module '@app-config' {
  import type { AppConfig } from './shared/lib/appConfig'
  export const appConfig: AppConfig
}

// Dev-only mock bridge (vite.config.ts alias): apps/<app>/dev/mock.ts.
declare module '@app-mock' {
  import type { ExecResult } from './shared/lib/ksu'
  export function mockExec(command: string): Promise<ExecResult>
}

// Build mode (vite.config.ts virtual module): `issuer` or `deployer`.
declare module 'virtual:app-mode' {
  export const APP_MODE: string
}
