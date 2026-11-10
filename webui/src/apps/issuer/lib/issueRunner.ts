// Issuance task: the issuer module's webroot/runner/issue.sh. Kept apart from
// the shared runner so shared code never imports issuer business types.
import { useTaskRunner, type TaskResult, type TaskRunner } from '../../../shared/lib/runner'
import { MODULE_DIR, TMP, type Envelope, type IssueSuccess } from './rkp'

export type IssueResult = TaskResult<Envelope<IssueSuccess>>
export type IssueRunner = TaskRunner<Envelope<IssueSuccess>>

export function useIssueRunner(onFinished: (result: IssueResult) => Promise<void> | void): IssueRunner {
  return useTaskRunner<Envelope<IssueSuccess>>(
    { script: `${MODULE_DIR}/webroot/runner/issue.sh`, tmp: TMP, name: 'issue', storageKey: 'rkp.issue' },
    onFinished,
  )
}
