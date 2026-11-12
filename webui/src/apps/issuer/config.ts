import type { AppConfig } from '../../shared/lib/appConfig'

// Issuer page: the rkp_mini_issuer module. Paths are data-compatibility
// identifiers and must not be renamed by the module split.
export const appConfig: AppConfig = {
  moduleId: 'rkp_mini_issuer',
  moduleDir: '/data/adb/modules/rkp_mini_issuer',
  dataRoot: '/data/misc/keystore/rkp-mini',
  syncRole: 'sender',
  loopScript: 'auto-loop.sh',
  loopLog: 'auto.log',
}
