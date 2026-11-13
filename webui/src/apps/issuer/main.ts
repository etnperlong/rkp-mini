import { createApp } from 'vue'
import 'miuix-vue/style.css'
import '../../shared/styles/app.css'
import { setupI18n, type LocaleModule } from '../../shared/lib/i18n'
import App from './App.vue'

void setupI18n('issuer', import.meta.glob<LocaleModule>('./locales/*.ts')).then((i18n) =>
  createApp(App).use(i18n).mount('#app'),
)
