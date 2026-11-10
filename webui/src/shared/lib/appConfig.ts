// Per-build configuration shared by both apps. Each app supplies its own
// values in `apps/<app>/config.ts`, resolved through the `@app-config` alias,
// so no shared module ever hardcodes a module id, install path or data root.
export interface AppConfig {
  /** KernelSU module id this page belongs to. */
  moduleId: string
  /** Fixed install path; KernelSU's `moduleInfo()` may override it. */
  moduleDir: string
  /** Data root: sender `/data/misc/keystore/rkp-mini`, receiver `/data/adb/rkp_sync`. */
  dataRoot: string
  /** Remote-sync role this app drives (data-compatibility identifier). */
  syncRole: 'sender' | 'receiver'
}
