// Typed message keys: `t('…')` with an unknown key fails `vue-tsc`. Each app
// bundle loads only `shared` plus its own tree at runtime.
import type shared from './shared/locales/zh-CN'
import type issuer from './apps/issuer/locales/zh-CN'
import type deployer from './apps/deployer/locales/zh-CN'

declare module 'vue-i18n' {
  export interface DefineLocaleMessage {
    shared: typeof shared
    issuer: typeof issuer
    deployer: typeof deployer
  }
}
