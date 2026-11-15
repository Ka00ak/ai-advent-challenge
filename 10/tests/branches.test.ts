import { describe, expect, it } from 'vitest'
import type { StickyFactsState } from '../src/agent/facts'
import {
  BRANCHES_STORAGE_KEY,
  activeBranch,
  clearBranches,
  createBranchState,
  createBranchesFromCheckpoint,
  loadBranches,
  parseBranches,
  saveBranches,
  saveCheckpoint,
  switchBranch,
  updateActiveBranch,
  type BranchMemorySnapshot,
} from '../src/memory/branches'
import type { KeyValueStorage } from '../src/storage/conversation'
import type { StoredSummary } from '../src/storage/summary'
import type { ChatMessage } from '../src/types'

class MemoryStorage implements KeyValueStorage {
  readonly data = new Map<string, string>()

  getItem(key: string): string | null {
    return this.data.get(key) ?? null
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value)
  }

  removeItem(key: string): void {
    this.data.delete(key)
  }
}

const messages: ChatMessage[] = [
  { role: 'user', content: 'Проект «Атлас», бюджет 480 000 рублей.' },
  { role: 'assistant', content: 'Записал.' },
]

const summary: StoredSummary = {
  version: 1,
  conversationId: 'conversation-1',
  text: '- Проект «Атлас»',
  coveredMessages: 2,
  updatedAt: '2026-11-15T10:00:00.000Z',
  mode: 'local',
  chunks: 1,
}

const facts: StickyFactsState = {
  facts: [
    {
      key: 'budget.total',
      value: '480 000 рублей',
      category: 'бюджет',
      updatedAt: '2026-11-15T10:00:00.000Z',
    },
  ],
  updatedAt: '2026-11-15T10:00:00.000Z',
  processedMessages: 2,
  updates: 1,
  mode: 'local',
}

function snapshot(): BranchMemorySnapshot {
  return {
    messages: messages.map(message => ({ ...message })),
    summary: { ...summary },
    facts: { ...facts, facts: facts.facts.map(fact => ({ ...fact })) },
  }
}

describe('ветвление диалога', () => {
  it('сохраняет checkpoint и создаёт две ветки от одной точки', () => {
    const base = createBranchState(snapshot(), 'Основная')
    const withCheckpoint = saveCheckpoint(base, snapshot(), 'Перед развилкой')

    expect(withCheckpoint.checkpoint?.messageIndex).toBe(2)
    expect(withCheckpoint.checkpoint?.facts?.facts[0]?.value).toBe('480 000 рублей')

    const { state, branchIds } = createBranchesFromCheckpoint(withCheckpoint)
    expect(branchIds).toHaveLength(2)
    expect(state.branches).toHaveLength(3)
    expect(state.activeBranchId).toBe(branchIds[0])
    expect(state.branches.find(branch => branch.id === branchIds[0])?.name).toBe('Ветка A')
    expect(state.branches.find(branch => branch.id === branchIds[1])?.name).toBe('Ветка B')
  })

  it('продолжает ветки независимо и переключается между ними', () => {
    const base = createBranchState(snapshot(), 'Основная')
    const checkpoint = saveCheckpoint(base, snapshot())
    const { state, branchIds } = createBranchesFromCheckpoint(checkpoint)
    const [aId, bId] = branchIds

    // В ветке A добавили решение и обновили facts.
    const branchA: BranchMemorySnapshot = {
      messages: [
        ...messages,
        { role: 'user', content: 'Решение: PostgreSQL.' },
        { role: 'assistant', content: 'Записал PostgreSQL.' },
      ],
      summary,
      facts: {
        ...facts,
        facts: [
          ...facts.facts,
          {
            key: 'decision.database',
            value: 'PostgreSQL',
            category: 'решение',
            updatedAt: '2026-11-15T11:00:00.000Z',
          },
        ],
      },
    }
    const afterA = updateActiveBranch(state, branchA)
    expect(activeBranch(afterA)?.messages).toHaveLength(4)

    // Ветка B осталась на checkpoint.
    const switchedB = switchBranch(afterA, bId!)
    expect(activeBranch(switchedB)?.id).toBe(bId)
    expect(activeBranch(switchedB)?.messages).toHaveLength(2)
    expect(activeBranch(switchedB)?.facts?.facts).toHaveLength(1)

    // Возврат в A видит её собственные изменения.
    const switchedA = switchBranch(switchedB, aId!)
    expect(activeBranch(switchedA)?.messages).toHaveLength(4)
    expect(activeBranch(switchedA)?.facts?.facts.map(fact => fact.key)).toContain(
      'decision.database',
    )
  })

  it('хранит ветки отдельно от истории и читает их обратно', () => {
    const storage = new MemoryStorage()
    const base = createBranchState(snapshot(), 'Основная')
    const checkpoint = saveCheckpoint(base, snapshot())
    const { state } = createBranchesFromCheckpoint(checkpoint)

    expect(saveBranches(state, storage).ok).toBe(true)
    expect(storage.getItem(BRANCHES_STORAGE_KEY)).toBeTruthy()
    expect(storage.getItem(BRANCHES_STORAGE_KEY)).not.toContain('lab10.conversation')

    const loaded = loadBranches(storage)
    expect(loaded.state?.branches).toHaveLength(3)
    expect(loaded.state?.checkpoint?.label).toBe('Checkpoint')
    expect(activeBranch(loaded.state)?.messages).toHaveLength(2)

    clearBranches(storage)
    expect(loadBranches(storage).state).toBeNull()
  })

  it('отбрасывает повреждённые ветки и неизвестную версию', () => {
    expect(parseBranches('{oops').error).toContain('повреждены')
    expect(parseBranches('null').error).toContain('JSON-объектом')
    expect(parseBranches(JSON.stringify({ version: 99 })).error).toContain(
      'Неподдерживаемая версия',
    )
    expect(parseBranches(JSON.stringify({ version: 1, branches: [] })).error).toContain(
      'нет ни одной',
    )
  })

  it('клонирует сообщения, чтобы ветки не делили один массив', () => {
    const source = snapshot()
    const base = createBranchState(source, 'Основная')
    source.messages.push({ role: 'user', content: 'Позднее сообщение' })

    expect(activeBranch(base)?.messages).toHaveLength(2)
  })
})
