import { fileURLToPath } from 'node:url'
import { defineConfig, type Plugin } from 'vite'
import vue from '@vitejs/plugin-vue'

// Production-only CSP: the dev server needs a websocket for HMR, which
// `connect-src 'none'` would block. Vue/motion write inline styles, so
// style-src keeps 'unsafe-inline'.
const CSP =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; " +
  "img-src 'self' data:; connect-src 'none'"

function csp(): Plugin {
  return {
    name: 'rkp-csp',
    apply: 'build',
    transformIndexHtml: () => [
      {
        tag: 'meta',
        attrs: { 'http-equiv': 'Content-Security-Policy', content: CSP },
        injectTo: 'head-prepend',
      },
    ],
  }
}

const APPS = ['issuer', 'deployer'] as const
type App = (typeof APPS)[number]

/** No default app: an empty or unknown mode is a configuration error. */
function resolveApp(mode: string): App {
  if ((APPS as readonly string[]).includes(mode)) return mode as App
  throw new Error(`未知的 WebUI 模式 "${mode}"：请使用 --mode issuer 或 --mode deployer`)
}

/**
 * `virtual:app-mode` exposes the validated mode to src/entry.ts in both dev
 * and build (Vite does not apply `define` to client sources in dev).
 */
function appMode(app: App): Plugin {
  const id = 'virtual:app-mode'
  const resolved = `\0${id}`
  return {
    name: 'rkp-app-mode',
    resolveId(source) {
      return source === id ? resolved : null
    },
    load(loadId) {
      return loadId === resolved ? `export const APP_MODE = ${JSON.stringify(app)}` : null
    },
  }
}

// Two apps from one source tree (design §19.5). Each explicit mode builds only
// its own app:
//   --mode issuer   → rkp_mini_issuer page  → dist/issuer/
//   --mode deployer → rkp_mini_deployer page → dist/deployer/
// The aliases pin the app entry, its config and its dev-only mock so neither
// bundle can contain the other app. Runner scripts live in the module trees;
// Vite ships no shell.
export default defineConfig(({ mode }) => {
  const app = resolveApp(mode)
  const appDir = fileURLToPath(new URL(`./src/apps/${app}`, import.meta.url))
  return {
    plugins: [vue(), csp(), appMode(app)],
    // vue-i18n bundler flags: Composition API only, no devtools in production.
    define: {
      __VUE_I18N_FULL_INSTALL__: false,
      __VUE_I18N_LEGACY_API__: false,
      __INTLIFY_PROD_DEVTOOLS__: false,
    },
    base: './', // KernelSU loads webroot via relative paths
    publicDir: false,
    resolve: {
      alias: {
        '@app': `${appDir}/main.ts`,
        '@app-config': `${appDir}/config.ts`,
        '@app-mock': `${appDir}/dev/mock.ts`,
      },
    },
    build: {
      outDir: `dist/${app}`,
      target: 'es2020',
      cssCodeSplit: false,
    },
  }
})
