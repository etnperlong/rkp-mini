import { createApp } from 'vue'
import 'miuix-vue/style.css'
import '../../shared/styles/app.css'
import { setupI18n, type LocaleModule } from '../../shared/lib/i18n'
import { syncModuleLocale } from '../../shared/lib/moduleLocale'
import { debug } from '../../shared/lib/debug'
import App from './App.vue'

void setupI18n('deployer', import.meta.glob<LocaleModule>('./locales/*.ts')).then((i18n) => {
  // App's first status read waits for this (shared/localeSyncSettled), so the
  // native description is refreshed with the locale the UI actually settled on.
  void syncModuleLocale().catch((e: unknown) => debug(`locale sync: ${e instanceof Error ? e.message : String(e)}`))
  createApp(App).use(i18n).mount('#app')
})
