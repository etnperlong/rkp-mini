import type { AppConfig } from '../../shared/lib/appConfig'

// Deployer page: the rkp_mini_deployer module. The receiver data root is a
// data-compatibility identifier and must not be renamed by the module split.
export const appConfig: AppConfig = {
  moduleId: 'rkp_mini_deployer',
  moduleDir: '/data/adb/modules/rkp_mini_deployer',
  dataRoot: '/data/adb/rkp_sync',
  syncRole: 'receiver',
}
