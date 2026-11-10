import { readFileSync } from 'node:fs'
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

/** Receiver build: ship only the sync runner, never the issuer's issue.sh. */
function receiverRunner(): Plugin {
  return {
    name: 'rkp-receiver-runner',
    apply: 'build',
    generateBundle() {
      this.emitFile({
        type: 'asset',
        fileName: 'runner/sync.sh',
        source: readFileSync(fileURLToPath(new URL('./public/runner/sync.sh', import.meta.url))),
      })
    },
  }
}

// Two apps from one source tree (design §19.5):
//   default mode   → sender page for the rkp_mini module   (dist/)
//   --mode receiver → receiver page for the rkp_sync module (dist-receiver/)
// `@app` resolves to the mode's entry so neither bundle contains the other.
export default defineConfig(({ mode }) => {
  const receiver = mode === 'receiver'
  const entry = receiver ? './src/receiver/main.ts' : './src/main.ts'
  return {
    plugins: [vue(), csp(), ...(receiver ? [receiverRunner()] : [])],
    base: './', // KernelSU loads webroot via relative paths
    publicDir: receiver ? false : 'public',
    resolve: {
      alias: { '@app': fileURLToPath(new URL(entry, import.meta.url)) },
    },
    build: {
      outDir: receiver ? 'dist-receiver' : 'dist',
      target: 'es2020',
      cssCodeSplit: false,
    },
  }
})
