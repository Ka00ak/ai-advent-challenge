import {
  isFactCategory,
  type FactMode,
  type StickyFact,
  type StickyFactsState,
} from '../agent/facts'
import { getBrowserStorage, type KeyValueStorage } from './conversation'

/** Key-value память хранится отдельно от истории и сводки. */
export const FACTS_STORAGE_KEY = 'lab10.facts.v1'
export const FACTS_VERSION = 1 as const

export interface StoredFacts extends StickyFactsState {
  version: typeof FACTS_VERSION
  /** К какой беседе относятся facts. null — беседа ещё не сохранена. */
  conversationId: string | null
}

export interface FactsLoadResult {
  facts: StoredFacts | null
  error?: string
}

export interface FactsSaveResult {
  ok: boolean
  error?: string
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

export function createStoredFacts(
  facts: StickyFactsState,
  conversationId: string | null,
): StoredFacts {
  return {
    version: FACTS_VERSION,
    conversationId,
    facts: facts.facts.map(fact => ({ ...fact })),
    updatedAt: facts.updatedAt,
    processedMessages: facts.processedMessages,
    updates: facts.updates,
    mode: facts.mode,
  }
}

function normalizeFact(value: unknown): StickyFact | null {
  const record = asRecord(value)
  if (!record) return null
  const key = typeof record.key === 'string' ? record.key.trim() : ''
  const factValue = typeof record.value === 'string' ? record.value.trim() : ''
  if (!key || !factValue) return null
  return {
    key,
    value: factValue,
    category: isFactCategory(record.category) ? record.category : 'другое',
    updatedAt:
      typeof record.updatedAt === 'string' && record.updatedAt
        ? record.updatedAt
        : new Date().toISOString(),
  }
}

export function parseFacts(raw: string): FactsLoadResult {
  if (!raw.trim()) return { facts: null, error: 'Facts пусты' }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (error) {
    return {
      facts: null,
      error: `Facts повреждены: ${error instanceof Error ? error.message : String(error)}`,
    }
  }

  const record = asRecord(parsed)
  if (!record) return { facts: null, error: 'Facts должны быть JSON-объектом' }
  if (record.version !== FACTS_VERSION) {
    return {
      facts: null,
      error: `Неподдерживаемая версия facts: ${String(record.version ?? '—')}`,
    }
  }

  if (!Array.isArray(record.facts)) {
    return { facts: null, error: 'В facts нет массива facts' }
  }

  const facts = record.facts
    .map(normalizeFact)
    .filter((fact): fact is StickyFact => fact !== null)

  const processedMessages =
    typeof record.processedMessages === 'number' &&
    Number.isFinite(record.processedMessages)
      ? Math.max(0, Math.floor(record.processedMessages))
      : 0
  const updates =
    typeof record.updates === 'number' && Number.isFinite(record.updates)
      ? Math.max(0, Math.floor(record.updates))
      : 0
  const mode: FactMode = record.mode === 'llm' ? 'llm' : 'local'

  return {
    facts: {
      version: FACTS_VERSION,
      conversationId:
        typeof record.conversationId === 'string' && record.conversationId
          ? record.conversationId
          : null,
      facts,
      updatedAt:
        typeof record.updatedAt === 'string' ? record.updatedAt : '',
      processedMessages,
      updates,
      mode,
    },
  }
}

export function loadFacts(
  storage: KeyValueStorage | null = getBrowserStorage(),
): FactsLoadResult {
  if (!storage) return { facts: null, error: 'localStorage недоступен в этом окружении' }

  let raw: string | null
  try {
    raw = storage.getItem(FACTS_STORAGE_KEY)
  } catch (error) {
    return {
      facts: null,
      error: `Не удалось прочитать facts: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }

  if (!raw) return { facts: null }
  return parseFacts(raw)
}

export function saveFacts(
  facts: StoredFacts,
  storage: KeyValueStorage | null = getBrowserStorage(),
): FactsSaveResult {
  if (!storage) return { ok: false, error: 'localStorage недоступен в этом окружении' }

  try {
    storage.setItem(FACTS_STORAGE_KEY, JSON.stringify(facts))
    return { ok: true }
  } catch (error) {
    return {
      ok: false,
      error: `Не удалось сохранить facts: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }
}

export function clearFacts(storage: KeyValueStorage | null = getBrowserStorage()): void {
  if (!storage) return
  try {
    storage.removeItem(FACTS_STORAGE_KEY)
  } catch {
    // Приватный режим — очищать нечего.
  }
}
