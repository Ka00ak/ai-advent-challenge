import { isFactCategory, type StickyFactsState } from '../agent/facts'
import type { StoredSummary } from '../storage/summary'
import { getBrowserStorage, type KeyValueStorage } from '../storage/conversation'
import type { ChatMessage } from '../types'

/**
 * Ветвление диалога.
 *
 * Checkpoint — снимок сообщений и памяти в конкретной точке. От него можно
 * создать две (и больше) ветки. Каждая ветка продолжает диалог независимо:
 * свои messages, своя сводка, свои facts. Переключение меняет активный снимок.
 */

export const BRANCHES_VERSION = 1 as const
export const BRANCHES_STORAGE_KEY = 'lab10.branches.v1'

export interface BranchMemorySnapshot {
  messages: ChatMessage[]
  summary: StoredSummary | null
  facts: StickyFactsState | null
}

export interface DialogCheckpoint extends BranchMemorySnapshot {
  id: string
  label: string
  createdAt: string
  /** Сколько сообщений было в истории в момент checkpoint. */
  messageIndex: number
}

export interface DialogBranch extends BranchMemorySnapshot {
  id: string
  name: string
  parentBranchId: string | null
  checkpointId: string | null
  createdAt: string
  updatedAt: string
}

export interface BranchState {
  version: typeof BRANCHES_VERSION
  activeBranchId: string
  checkpoint: DialogCheckpoint | null
  branches: DialogBranch[]
}

export interface BranchListResult {
  state: BranchState | null
  error?: string
}

function makeId(prefix: string): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${prefix}-${crypto.randomUUID()}`
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

function cloneSnapshot(snapshot: BranchMemorySnapshot): BranchMemorySnapshot {
  return {
    messages: clone(snapshot.messages),
    summary: snapshot.summary ? clone(snapshot.summary) : null,
    facts: snapshot.facts ? clone(snapshot.facts) : null,
  }
}

function branchFromSnapshot(
  snapshot: BranchMemorySnapshot,
  name: string,
  options: {
    parentBranchId?: string | null
    checkpointId?: string | null
    now?: Date
  } = {},
): DialogBranch {
  const timestamp = (options.now ?? new Date()).toISOString()
  return {
    id: makeId('branch'),
    name,
    parentBranchId: options.parentBranchId ?? null,
    checkpointId: options.checkpointId ?? null,
    createdAt: timestamp,
    updatedAt: timestamp,
    ...cloneSnapshot(snapshot),
  }
}

export function createBranchState(
  snapshot: BranchMemorySnapshot = { messages: [], summary: null, facts: null },
  name = 'Основная',
  now = new Date(),
): BranchState {
  const branch = branchFromSnapshot(snapshot, name, { now })
  return {
    version: BRANCHES_VERSION,
    activeBranchId: branch.id,
    checkpoint: null,
    branches: [branch],
  }
}

export function activeBranch(state: BranchState | null): DialogBranch | null {
  if (!state) return null
  return (
    state.branches.find(branch => branch.id === state.activeBranchId) ??
    state.branches[0] ??
    null
  )
}

export function saveCheckpoint(
  state: BranchState,
  snapshot: BranchMemorySnapshot,
  label = 'Checkpoint',
  now = new Date(),
): BranchState {
  const checkpoint: DialogCheckpoint = {
    id: makeId('checkpoint'),
    label,
    createdAt: now.toISOString(),
    messageIndex: snapshot.messages.length,
    ...cloneSnapshot(snapshot),
  }
  return { ...state, checkpoint }
}

/**
 * Создаёт две ветки от checkpoint. Первая становится активной. Имена
 * разводятся, если такие уже есть: «Ветка A», «Ветка B», «Ветка A (2)»…
 */
export function createBranchesFromCheckpoint(
  state: BranchState,
  names: [string, string] = ['Ветка A', 'Ветка B'],
  now = new Date(),
): { state: BranchState; branchIds: string[] } {
  const checkpoint = state.checkpoint
  if (!checkpoint) return { state, branchIds: [] }

  const existingNames = new Set(state.branches.map(branch => branch.name))
  const uniqueName = (base: string): string => {
    if (!existingNames.has(base)) {
      existingNames.add(base)
      return base
    }
    let index = 2
    while (existingNames.has(`${base} (${index})`)) index += 1
    const name = `${base} (${index})`
    existingNames.add(name)
    return name
  }

  const branches = names.map(name =>
    branchFromSnapshot(checkpoint, uniqueName(name), {
      parentBranchId: null,
      checkpointId: checkpoint.id,
      now,
    }),
  )

  return {
    state: {
      ...state,
      activeBranchId: branches[0]!.id,
      branches: [...state.branches, ...branches],
    },
    branchIds: branches.map(branch => branch.id),
  }
}

export function switchBranch(state: BranchState, branchId: string): BranchState {
  if (!state.branches.some(branch => branch.id === branchId)) return state
  return { ...state, activeBranchId: branchId }
}

export function updateBranch(
  state: BranchState,
  branchId: string,
  snapshot: BranchMemorySnapshot,
  now = new Date(),
): BranchState {
  const branches = state.branches.map(branch => {
    if (branch.id !== branchId) return branch
    return {
      ...branch,
      ...cloneSnapshot(snapshot),
      updatedAt: now.toISOString(),
    }
  })
  return { ...state, branches }
}

export function updateActiveBranch(
  state: BranchState,
  snapshot: BranchMemorySnapshot,
  now = new Date(),
): BranchState {
  return updateBranch(state, state.activeBranchId, snapshot, now)
}

export function removeBranch(state: BranchState, branchId: string): BranchState {
  if (state.branches.length <= 1) return state
  const branches = state.branches.filter(branch => branch.id !== branchId)
  return {
    ...state,
    branches,
    activeBranchId:
      state.activeBranchId === branchId ? branches[0]!.id : state.activeBranchId,
  }
}

/* ------------------------------------------------------------------ */
/* localStorage                                                        */
/* ------------------------------------------------------------------ */

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

function normalizeMessages(value: unknown): ChatMessage[] {
  if (!Array.isArray(value)) return []
  return value
    .map((item): ChatMessage | null => {
      const record = asRecord(item)
      if (!record) return null
      if (record.role !== 'user' && record.role !== 'assistant' && record.role !== 'tool') {
        return null
      }
      const message: ChatMessage = {
        role: record.role,
        content: typeof record.content === 'string' ? record.content : null,
      }
      if (Array.isArray(record.tool_calls)) {
        message.tool_calls = clone(record.tool_calls) as ChatMessage['tool_calls']
      }
      if (typeof record.tool_call_id === 'string') message.tool_call_id = record.tool_call_id
      if (typeof record.name === 'string') message.name = record.name
      return message
    })
    .filter((message): message is ChatMessage => message !== null)
}

function normalizeSummary(value: unknown): StoredSummary | null {
  const record = asRecord(value)
  if (!record || typeof record.text !== 'string' || !record.text.trim()) return null
  return {
    version: 1,
    conversationId:
      typeof record.conversationId === 'string' ? record.conversationId : null,
    text: record.text,
    coveredMessages:
      typeof record.coveredMessages === 'number' ? record.coveredMessages : 0,
    updatedAt:
      typeof record.updatedAt === 'string'
        ? record.updatedAt
        : new Date().toISOString(),
    mode: record.mode === 'local' ? 'local' : 'llm',
    chunks: typeof record.chunks === 'number' ? record.chunks : 1,
  }
}

function normalizeFacts(value: unknown): StickyFactsState | null {
  const record = asRecord(value)
  if (!record || !Array.isArray(record.facts)) return null
  return {
    facts: record.facts
      .map(item => {
        const fact = asRecord(item)
        if (!fact || typeof fact.key !== 'string' || typeof fact.value !== 'string') {
          return null
        }
        return {
          key: fact.key,
          value: fact.value,
          category: isFactCategory(fact.category) ? fact.category : 'другое',
          updatedAt:
            typeof fact.updatedAt === 'string'
              ? fact.updatedAt
              : new Date().toISOString(),
        }
      })
      .filter((fact): fact is NonNullable<typeof fact> => fact !== null),
    updatedAt: typeof record.updatedAt === 'string' ? record.updatedAt : '',
    processedMessages:
      typeof record.processedMessages === 'number' ? record.processedMessages : 0,
    updates: typeof record.updates === 'number' ? record.updates : 0,
    mode: record.mode === 'llm' ? 'llm' : 'local',
  }
}

function normalizeBranch(value: unknown): DialogBranch | null {
  const record = asRecord(value)
  if (!record) return null
  if (typeof record.id !== 'string' || typeof record.name !== 'string') return null
  return {
    id: record.id,
    name: record.name,
    parentBranchId:
      typeof record.parentBranchId === 'string' ? record.parentBranchId : null,
    checkpointId:
      typeof record.checkpointId === 'string' ? record.checkpointId : null,
    createdAt:
      typeof record.createdAt === 'string'
        ? record.createdAt
        : new Date().toISOString(),
    updatedAt:
      typeof record.updatedAt === 'string'
        ? record.updatedAt
        : new Date().toISOString(),
    messages: normalizeMessages(record.messages),
    summary: normalizeSummary(record.summary),
    facts: normalizeFacts(record.facts),
  }
}

export function parseBranches(raw: string): BranchListResult {
  if (!raw.trim()) return { state: null, error: 'Ветки пусты' }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (error) {
    return {
      state: null,
      error: `Ветки повреждены: ${error instanceof Error ? error.message : String(error)}`,
    }
  }

  const record = asRecord(parsed)
  if (!record) return { state: null, error: 'Ветки должны быть JSON-объектом' }
  if (record.version !== BRANCHES_VERSION) {
    return {
      state: null,
      error: `Неподдерживаемая версия веток: ${String(record.version ?? '—')}`,
    }
  }
  if (!Array.isArray(record.branches)) {
    return { state: null, error: 'В ветках нет массива branches' }
  }

  const branches = record.branches
    .map(normalizeBranch)
    .filter((branch): branch is DialogBranch => branch !== null)
  if (!branches.length) {
    return { state: null, error: 'В ветках нет ни одной корректной записи' }
  }

  const activeId =
    typeof record.activeBranchId === 'string' &&
    branches.some(branch => branch.id === record.activeBranchId)
      ? record.activeBranchId
      : branches[0]!.id

  const rawCheckpoint = asRecord(record.checkpoint)
  const checkpoint: DialogCheckpoint | null = rawCheckpoint
    ? {
        id:
          typeof rawCheckpoint.id === 'string'
            ? rawCheckpoint.id
            : makeId('checkpoint'),
        label:
          typeof rawCheckpoint.label === 'string'
            ? rawCheckpoint.label
            : 'Checkpoint',
        createdAt:
          typeof rawCheckpoint.createdAt === 'string'
            ? rawCheckpoint.createdAt
            : new Date().toISOString(),
        messageIndex:
          typeof rawCheckpoint.messageIndex === 'number'
            ? rawCheckpoint.messageIndex
            : 0,
        messages: normalizeMessages(rawCheckpoint.messages),
        summary: normalizeSummary(rawCheckpoint.summary),
        facts: normalizeFacts(rawCheckpoint.facts),
      }
    : null

  return {
    state: {
      version: BRANCHES_VERSION,
      activeBranchId: activeId,
      checkpoint,
      branches,
    },
  }
}

export function loadBranches(
  storage: KeyValueStorage | null = getBrowserStorage(),
): BranchListResult {
  if (!storage) return { state: null, error: 'localStorage недоступен в этом окружении' }

  let raw: string | null
  try {
    raw = storage.getItem(BRANCHES_STORAGE_KEY)
  } catch (error) {
    return {
      state: null,
      error: `Не удалось прочитать ветки: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }

  if (!raw) return { state: null }
  return parseBranches(raw)
}

export function saveBranches(
  state: BranchState,
  storage: KeyValueStorage | null = getBrowserStorage(),
): { ok: boolean; error?: string } {
  if (!storage) return { ok: false, error: 'localStorage недоступен в этом окружении' }
  try {
    storage.setItem(BRANCHES_STORAGE_KEY, JSON.stringify(state))
    return { ok: true }
  } catch (error) {
    return {
      ok: false,
      error: `Не удалось сохранить ветки: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }
}

export function clearBranches(storage: KeyValueStorage | null = getBrowserStorage()): void {
  if (!storage) return
  try {
    storage.removeItem(BRANCHES_STORAGE_KEY)
  } catch {
    // ignore
  }
}
