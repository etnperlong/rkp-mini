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

export default defineConfig({
  plugins: [vue(), csp()],
  base: './', // KernelSU loads webroot via relative paths
  build: {
    outDir: 'dist',
    target: 'es2020',
    cssCodeSplit: false,
  },
})
