import type { ContextSummary, SummaryMode } from '../agent/compression'
import { getBrowserStorage, type KeyValueStorage } from './conversation'

/** Сводка хранится отдельно от полной истории диалога. */
export const SUMMARY_STORAGE_KEY = 'lab09.summary.v1'
export const SUMMARY_VERSION = 1 as const

export interface StoredSummary extends ContextSummary {
  version: typeof SUMMARY_VERSION
  /** К какой беседе относится сводка. null — беседа ещё не сохранена. */
  conversationId: string | null
}

export interface SummaryLoadResult {
  summary: StoredSummary | null
  error?: string
}

export interface SummarySaveResult {
  ok: boolean
  error?: string
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

export function createStoredSummary(
  summary: ContextSummary,
  conversationId: string | null,
): StoredSummary {
  return {
    version: SUMMARY_VERSION,
    conversationId,
    text: summary.text,
    coveredMessages: summary.coveredMessages,
    updatedAt: summary.updatedAt,
    mode: summary.mode,
    chunks: summary.chunks,
  }
}

/** Разбирает и валидирует отдельный JSON со сводкой. */
export function parseSummary(raw: string): SummaryLoadResult {
  if (!raw.trim()) return { summary: null, error: 'Сводка пуста' }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (error) {
    return {
      summary: null,
      error: `Сводка повреждена: ${error instanceof Error ? error.message : String(error)}`,
    }
  }

  const record = asRecord(parsed)
  if (!record) return { summary: null, error: 'Сводка должна быть JSON-объектом' }
  if (record.version !== SUMMARY_VERSION) {
    return {
      summary: null,
      error: `Неподдерживаемая версия сводки: ${String(record.version ?? '—')}`,
    }
  }

  const text = typeof record.text === 'string' ? record.text.trim() : ''
  if (!text) return { summary: null, error: 'В сводке нет текста' }

  const coveredMessages =
    typeof record.coveredMessages === 'number' && Number.isFinite(record.coveredMessages)
      ? Math.max(0, Math.floor(record.coveredMessages))
      : 0
  const chunks =
    typeof record.chunks === 'number' && Number.isFinite(record.chunks)
      ? Math.max(1, Math.floor(record.chunks))
      : 1
  const mode: SummaryMode = record.mode === 'local' ? 'local' : 'llm'

  return {
    summary: {
      version: SUMMARY_VERSION,
      conversationId:
        typeof record.conversationId === 'string' && record.conversationId
          ? record.conversationId
          : null,
      text,
      coveredMessages,
      updatedAt:
        typeof record.updatedAt === 'string' && record.updatedAt
          ? record.updatedAt
          : new Date().toISOString(),
      mode,
      chunks,
    },
  }
}

export function loadSummary(
  storage: KeyValueStorage | null = getBrowserStorage(),
): SummaryLoadResult {
  if (!storage) return { summary: null, error: 'localStorage недоступен в этом окружении' }

  let raw: string | null
  try {
    raw = storage.getItem(SUMMARY_STORAGE_KEY)
  } catch (error) {
    return {
      summary: null,
      error: `Не удалось прочитать сводку: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }

  if (!raw) return { summary: null }
  return parseSummary(raw)
}

export function saveSummary(
  summary: StoredSummary,
  storage: KeyValueStorage | null = getBrowserStorage(),
): SummarySaveResult {
  if (!storage) return { ok: false, error: 'localStorage недоступен в этом окружении' }

  try {
    storage.setItem(SUMMARY_STORAGE_KEY, JSON.stringify(summary))
    return { ok: true }
  } catch (error) {
    return {
      ok: false,
      error: `Не удалось сохранить сводку: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }
}

export function clearSummary(storage: KeyValueStorage | null = getBrowserStorage()): void {
  if (!storage) return
  try {
    storage.removeItem(SUMMARY_STORAGE_KEY)
  } catch {
    // Приватный режим или запрет на запись — очищать нечего.
  }
}
