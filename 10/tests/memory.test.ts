import { afterEach, describe, expect, it, vi } from 'vitest'
import { ALL_BENCHMARK_MODES, runMemoryBenchmark } from '../src/memory/benchmark'
import {
  MEMORY_DETAILS,
  answerMentions,
  buildMemoryDialog,
  messagesContainDetail,
} from '../src/memory/dialog'
import {
  SUMMARY_STORAGE_KEY,
  clearSummary,
  createStoredSummary,
  loadSummary,
  parseSummary,
  saveSummary,
} from '../src/storage/summary'
import type { KeyValueStorage } from '../src/storage/conversation'
import { resolveModelProfile } from '../src/tokens'
import type { LlmRequest } from '../src/types'

class MemoryStorage implements KeyValueStorage {
  private readonly data = new Map<string, string>()

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

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('контрольный диалог', () => {
  it('содержит 24 сообщения и 12 деталей в начале', () => {
    const dialog = buildMemoryDialog()

    expect(dialog).toHaveLength(24)
    expect(MEMORY_DETAILS).toHaveLength(12)

    for (const detail of MEMORY_DETAILS) {
      expect(messagesContainDetail(dialog.slice(0, 16), detail)).toBe(true)
    }
  })

  it('в последних 8 сообщениях нет ни одной контрольной детали', () => {
    const dialog = buildMemoryDialog()
    const recent = dialog.slice(-8)

    for (const detail of MEMORY_DETAILS) {
      expect(messagesContainDetail(recent, detail)).toBe(false)
    }
  })

  it('распознаёт детали в ответе модели', () => {
    const budget = MEMORY_DETAILS.find(detail => detail.id === 'budget')!
    const customer = MEMORY_DETAILS.find(detail => detail.id === 'customer')!
    const deadline = MEMORY_DETAILS.find(detail => detail.id === 'deadline')!

    expect(answerMentions(budget, 'Бюджет проекта — 480 000 рублей.')).toBe(true)
    expect(answerMentions(budget, '480 тысяч рублей')).toBe(true)
    expect(answerMentions(budget, 'не знаю')).toBe(false)
    expect(answerMentions(customer, 'Заказчик — Марина Ковалёва.')).toBe(true)
    expect(answerMentions(customer, 'Заказчик — Ирина.')).toBe(false)
    expect(answerMentions(deadline, 'Дедлайн 15 декабря 2026 года.')).toBe(true)
  })
})

describe('проверка памяти через API', () => {
  it('сравнивает полную историю и сжатие: детали и токены', async () => {
    const dialog = buildMemoryDialog()
    const profile = resolveModelProfile('deepseek-chat')
    const requests: LlmRequest[] = []

    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      const request = JSON.parse(String(init?.body)) as LlmRequest
      requests.push(request)
      const lastUser = [...request.messages].reverse().find(message => message.role === 'user')
      const detail = MEMORY_DETAILS.find(item => item.question === lastUser?.content)
      return jsonResponse({
        choices: [
          {
            index: 0,
            message: { role: 'assistant', content: detail?.fact ?? 'не знаю' },
            finish_reason: 'stop',
          },
        ],
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const report = await runMemoryBenchmark({
      messages: dialog,
      details: MEMORY_DETAILS,
      settings: {
        baseUrl: 'https://example.test/v1',
        apiKey: 'test-key',
        model: 'deepseek-chat',
        temperature: 0,
        maxTokens: null,
        maxIterations: 3,
      },
      profile,
      compression: {
        enabled: true,
        keepRecent: 8,
        summarizeEvery: 10,
        mode: 'local',
      },
      summary: null,
    })

    expect(fetchMock).toHaveBeenCalledTimes(24)
    expect(report.full.remembered).toBe(12)
    expect(report.compressed.remembered).toBe(12)
    expect(report.compressed.historyTokens).toBeLessThan(report.full.historyTokens)
    expect(report.savedPromptTokens).toBeGreaterThan(0)
    expect(report.netSavedPromptTokens).toBeGreaterThan(0)
    expect(report.compression.usedSummary).toBe(true)
    expect(report.summary?.coveredMessages).toBe(16)

    const compressedRequest = requests[12]
    expect(compressedRequest?.messages[1]?.role).toBe('system')
    expect(compressedRequest?.messages[1]?.content).toContain('Сводка предыдущей части')
    expect(compressedRequest?.messages.length).toBeLessThan(
      requests[0]?.messages.length ?? 0,
    )
  })

  it('прогоняет sliding, facts и summary на одном диалоге и сравнивает детали', async () => {
    const dialog = buildMemoryDialog()
    const profile = resolveModelProfile('deepseek-chat')
    const requests: LlmRequest[] = []

    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      const request = JSON.parse(String(init?.body)) as LlmRequest
      requests.push(request)
      const lastUser = [...request.messages].reverse().find(message => message.role === 'user')
      const detail = MEMORY_DETAILS.find(item => item.question === lastUser?.content)
      // Модель отвечает только тем, что реально видит в контексте.
      const visible = detail ? messagesContainDetail(request.messages, detail) : false
      return jsonResponse({
        choices: [
          {
            index: 0,
            message: {
              role: 'assistant',
              content: visible ? detail!.fact : 'не знаю',
            },
            finish_reason: 'stop',
          },
        ],
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const report = await runMemoryBenchmark({
      messages: dialog,
      details: MEMORY_DETAILS,
      settings: {
        baseUrl: 'https://example.test/v1',
        apiKey: 'test-key',
        model: 'deepseek-chat',
        temperature: 0,
        maxTokens: null,
        maxIterations: 3,
      },
      profile,
      compression: {
        enabled: true,
        keepRecent: 8,
        summarizeEvery: 10,
        mode: 'local',
      },
      summary: null,
      facts: null,
      factsMode: 'local',
      modes: ALL_BENCHMARK_MODES,
    })

    // 4 режима × 12 вопросов, без отдельных запросов на память в local-режиме.
    expect(fetchMock).toHaveBeenCalledTimes(48)
    expect(report.strategies).toHaveLength(4)
    expect(report.modes).toEqual(ALL_BENCHMARK_MODES)

    expect(report.full.remembered).toBe(12)
    expect(report.sliding?.remembered).toBe(0)
    expect(report.facts?.remembered).toBe(12)
    expect(report.compressed.remembered).toBe(12)

    expect(report.facts?.factsCount).toBeGreaterThanOrEqual(12)
    expect(report.facts?.extractionRequests).toBe(0)
    expect(report.sliding?.extractionRequests).toBe(0)
    expect(report.facts?.contextTokens).toBeLessThan(report.full.contextTokens)
    expect(report.sliding?.contextTokens).toBeLessThan(report.full.contextTokens)
    expect(report.factsState?.facts.length).toBeGreaterThanOrEqual(12)
  })
})

describe('хранилище сводки', () => {
  it('сохраняет и загружает сводку отдельно от истории', () => {
    const storage = new MemoryStorage()
    const summary = createStoredSummary(
      {
        text: '- Дедлайн: 15 декабря 2026.',
        coveredMessages: 16,
        updatedAt: '2026-11-15T12:00:00.000Z',
        mode: 'local',
        chunks: 2,
      },
      'conversation-1',
    )

    expect(saveSummary(summary, storage).ok).toBe(true)
    expect(storage.getItem(SUMMARY_STORAGE_KEY)).toBeTruthy()
    expect(storage.getItem(SUMMARY_STORAGE_KEY)).not.toContain('messages')

    const loaded = loadSummary(storage)
    expect(loaded.summary).toEqual(summary)
    expect(loaded.summary?.coveredMessages).toBe(16)
    expect(loaded.summary?.chunks).toBe(2)
  })

  it('очищает сводку', () => {
    const storage = new MemoryStorage()
    const summary = createStoredSummary(
      {
        text: 'текст',
        coveredMessages: 10,
        updatedAt: '2026-11-15T12:00:00.000Z',
        mode: 'llm',
        chunks: 1,
      },
      null,
    )
    saveSummary(summary, storage)
    clearSummary(storage)

    expect(loadSummary(storage).summary).toBeNull()
  })

  it('отбрасывает повреждённую сводку и неизвестную версию', () => {
    expect(parseSummary('{oops').error).toContain('повреждена')
    expect(parseSummary('null').error).toContain('JSON-объектом')
    expect(parseSummary(JSON.stringify({ version: 99, text: 'x' })).error).toContain(
      'Неподдерживаемая версия',
    )
    expect(parseSummary(JSON.stringify({ version: 1, text: '' })).error).toContain(
      'нет текста',
    )
  })
})
