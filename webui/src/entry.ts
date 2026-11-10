// Explicit build-mode entry: `vite --mode issuer|deployer` validates the mode
// in vite.config.ts and exposes it through `virtual:app-mode`; the `@app` alias
// resolves to that app's main. There is no default app: an empty or unknown
// mode is a configuration error, never a silent fallback.
import { APP_MODE } from 'virtual:app-mode'

function appEntry(): Promise<unknown> {
  switch (APP_MODE) {
    case 'issuer':
    case 'deployer':
      return import('@app')
    default:
      throw new Error(
        `未知的 WebUI 模式 "${APP_MODE}"：请使用 vite --mode issuer 或 vite --mode deployer`,
      )
  }
}

appEntry().catch((e: unknown) => {
  console.error('WebUI 启动失败：', e)
})
