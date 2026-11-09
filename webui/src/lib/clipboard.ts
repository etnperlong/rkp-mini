import { showSnackbar } from 'miuix-vue'

function legacyCopy(text: string): boolean {
  const ta = document.createElement('textarea')
  ta.value = text
  ta.setAttribute('readonly', '')
  ta.style.position = 'fixed'
  ta.style.opacity = '0'
  document.body.appendChild(ta)
  ta.select()
  try {
    return document.execCommand('copy')
  } catch {
    return false
  } finally {
    ta.remove()
  }
}

/** Copy to clipboard (async API, then execCommand fallback) and report via snackbar. */
export async function copyText(text: string, label = '内容'): Promise<void> {
  let copied = false
  try {
    await navigator.clipboard.writeText(text)
    copied = true
  } catch {
    copied = legacyCopy(text)
  }
  void showSnackbar({ message: copied ? `已复制${label}` : `复制失败，请长按文本手动复制` })
}
