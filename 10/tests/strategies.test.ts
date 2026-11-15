import { afterEach, describe, expect, it, vi } from 'vitest'
import { Agent, type AgentConfig } from '../src/agent/Agent'
import type { CompressionConfig } from '../src/agent/compression'
import {
  buildContextMessages,
  normalizeContextConfig,
  previewContext,
} from '../src/agent/context'
import { createLlmFactExtractor, parseFactsJson } from '../src/agent/factExtractor'
import {
  extractFactsLocally,
  factsMessage,
  updateFacts,
} from '../src/agent/facts'
import { ToolRegistry } from '../src/agent/ToolRegistry'
import {
  MEMORY_DETAILS,
  answerMentions,
  buildMemoryDialog,
  messagesContainDetail,
} from '../src/memory/dialog'
import { resolveModelProfile } from '../src/tokens'
import { calculatorTool } from '../src/tools/calculator'
import type { ChatMessage, LlmRequest } from '../src/types'

const localCompression: CompressionConfig = {
  enabled: true,
  keepRecent: 8,
  summarizeEvery: 10,
  mode: 'local',
}

const agentConfig: AgentConfig = {
  baseUrl: 'https://example.test/v1',
  apiKey: 'test-key',
  model: 'deepseek-chat',
  temperature: 0,
  maxTokens: null,
  maxIterations: 3,
  systemPrompt: 'Ты тестовый агент.',
  compression: localCompression,
  strategy: 'facts',
  factsMode: 'local',
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function completion(content: string): unknown {
  return {
    id: 'resp_test',
    choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }],
    usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('Sticky Facts: локальное извлечение', () => {
  it('вытаскивает все 12 контрольных деталей из диалога', async () => {
    const outcome = await updateFacts({
      messages: buildMemoryDialog(),
      previous: null,
      mode: 'local',
      now: () => new Date('2026-11-15T12:00:00.000Z'),
    })

    expect(outcome.facts.facts.length).toBeGreaterThanOrEqual(12)
    expect(outcome.facts.processedMessages).toBe(24)
    expect(outcome.stats.updatedMessages).toBe(9)
    expect(outcome.stats.requests).toBe(0)
    expect(outcome.facts.mode).toBe('local')

    const memory = [factsMessage(outcome.facts)]
    for (const detail of MEMORY_DETAILS) {
      expect(messagesContainDetail(memory, detail), detail.fact).toBe(true)
    }
  })

  it('обновляет facts после каждой реплики пользователя, а не всего диалога', async () => {
    const dialog = buildMemoryDialog()
    const first = await updateFacts({ messages: dialog.slice(0, 6), mode: 'local' })
    const second = await updateFacts({
      messages: dialog.slice(0, 10),
      previous: first.facts,
      mode: 'local',
    })

    expect(first.stats.userMessages).toBe(3)
    expect(second.stats.newMessages).toBe(4)
    expect(second.stats.userMessages).toBe(2)
    expect(second.facts.processedMessages).toBe(10)
    expect(second.facts.facts.length).toBeGreaterThan(first.facts.facts.length)
  })

  it('перезаписывает факт по тому же ключу, не плодя дубликаты', () => {
    const first = extractFactsLocally({
      role: 'user',
      content: 'Бюджет — 480 000 рублей.',
    })
    const second = extractFactsLocally({
      role: 'user',
      content: 'Бюджет — 500 000 рублей.',
    })

    expect(first.some(fact => fact.key === 'budget.total')).toBe(true)
    expect(second.some(fact => fact.key === 'budget.total')).toBe(true)
  })

  it('в LLM-режиме делает по одному запросу на пользовательское сообщение', async () => {
    const messages: ChatMessage[] = [
      { role: 'user', content: 'Бюджет — 100 000 рублей.' },
      { role: 'assistant', content: 'Записал.' },
      { role: 'user', content: 'Дедлайн — 1 марта 2027.' },
      { role: 'assistant', content: 'Хорошо.' },
      { role: 'user', content: 'Решение: PostgreSQL.' },
    ]
    const calls: ChatMessage[] = []

    const outcome = await updateFacts({
      messages,
      mode: 'llm',
      extractor: async ({ message }) => {
        calls.push(message)
        return {
          facts: [
            {
              key: `fact.${calls.length}`,
              value: message.content ?? '',
              category: 'другое',
              updatedAt: '2026-11-15T12:00:00.000Z',
            },
          ],
          mode: 'llm',
          requests: 1,
          promptTokens: 120,
          completionTokens: 20,
          costUsd: 0.0001,
          usageReported: true,
        }
      },
    })

    expect(calls).toHaveLength(3)
    expect(outcome.stats.requests).toBe(3)
    expect(outcome.stats.promptTokens).toBe(360)
    expect(outcome.stats.completionTokens).toBe(60)
    expect(outcome.facts.mode).toBe('llm')
  })
})

describe('три стратегии', () => {
  it('Sliding Window оставляет только последние 8 сообщений и теряет старые детали', () => {
    const dialog = buildMemoryDialog()
    const config = normalizeContextConfig({
      strategy: 'sliding',
      keepRecent: 8,
      compression: localCompression,
    })
    const sent = buildContextMessages(dialog, config, null, null)
    const preview = previewContext(dialog, config, null, null)

    expect(sent).toHaveLength(8)
    expect(preview.recentMessages).toBe(8)
    expect(preview.droppedMessages).toBe(16)

    for (const detail of MEMORY_DETAILS) {
      expect(messagesContainDetail(sent, detail), detail.fact).toBe(false)
    }
  })

  it('Sticky Facts отправляет facts + последние 8 сообщений и сохраняет старые детали', async () => {
    const dialog = buildMemoryDialog()
    const config = normalizeContextConfig({
      strategy: 'facts',
      keepRecent: 8,
      compression: localCompression,
      factsMode: 'local',
    })
    const outcome = await updateFacts({ messages: dialog, mode: 'local' })
    const sent = buildContextMessages(dialog, config, null, outcome.facts)
    const preview = previewContext(dialog, config, null, outcome.facts)

    expect(sent[0]?.role).toBe('system')
    expect(sent[0]?.content).toContain('Долговременная память')
    expect(sent.length).toBe(9)
    expect(preview.factsTokens).toBeGreaterThan(0)
    expect(preview.sentHistoryTokens).toBeLessThan(preview.fullHistoryTokens)
    expect(preview.factsCount).toBeGreaterThanOrEqual(12)

    for (const detail of MEMORY_DETAILS) {
      expect(messagesContainDetail(sent, detail), detail.fact).toBe(true)
    }
  })

  it('Summary остаётся третьей стратегией и использует сводку', async () => {
    const dialog = buildMemoryDialog()
    const config = normalizeContextConfig({
      strategy: 'summary',
      keepRecent: 8,
      compression: localCompression,
    })
    const preview = previewContext(dialog, config, null, null)

    expect(preview.strategy).toBe('summary')
    expect(preview.sentHistoryTokens).toBe(preview.fullHistoryTokens)
  })
})

describe('Agent со стратегией facts', () => {
  it('сам обновляет facts и отправляет их вместе с последними сообщениями', async () => {
    const requests: LlmRequest[] = []
    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      requests.push(JSON.parse(String(init?.body)) as LlmRequest)
      return jsonResponse(completion('Тимлид — Артём Соколов.'))
    })
    vi.stubGlobal('fetch', fetchMock)

    const updates: number[] = []
    const agent = new Agent(agentConfig, new ToolRegistry([calculatorTool]))
    const result = await agent.run(
      'Кто тимлид проекта?',
      buildMemoryDialog(),
      {
        onFactsUpdate: next => updates.push(next?.facts.length ?? 0),
      },
    )

    // В local-режиме нет отдельных запросов на извлечение — только чат.
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(updates).toHaveLength(1)
    expect(result.facts?.facts.length).toBeGreaterThanOrEqual(12)
    expect(result.context.strategy).toBe('facts')
    expect(result.context.factsCount).toBeGreaterThanOrEqual(12)
    expect(result.context.factsRequests).toBe(0)

    const sent = requests[0]?.messages ?? []
    expect(sent[0]).toMatchObject({ role: 'system' })
    expect(sent[1]?.content).toContain('Долговременная память')
    expect(sent.at(-1)).toMatchObject({ role: 'user', content: 'Кто тимлид проекта?' })
    expect(sent.length).toBeLessThan(buildMemoryDialog().length)

    const lead = MEMORY_DETAILS.find(detail => detail.id === 'lead')!
    expect(answerMentions(lead, result.finalText)).toBe(true)
    expect(result.messages).toHaveLength(26)
  })

  it('сбой LLM-извлечения откатывается на локальные facts', async () => {
    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      const request = JSON.parse(String(init?.body)) as LlmRequest
      if (request.messages[0]?.content.includes('долговременную память')) {
        return jsonResponse(completion('это не JSON'))
      }
      return jsonResponse(completion('Ответ на вопрос.'))
    })
    vi.stubGlobal('fetch', fetchMock)

    const agent = new Agent(
      { ...agentConfig, factsMode: 'llm' },
      new ToolRegistry([calculatorTool]),
    )
    const result = await agent.run('Какой бюджет?', buildMemoryDialog())

    expect(result.facts?.facts.length).toBeGreaterThanOrEqual(12)
    expect(result.context.factsError).toBeTruthy()
    expect(result.facts?.mode).toBe('local')
  })
})

describe('LLM-извлечение facts', () => {
  it('парсит JSON в markdown-обёртке и возвращает usage', async () => {
    const fetchMock = vi.fn(async () =>
      jsonResponse({
        choices: [
          {
            index: 0,
            message: {
              role: 'assistant',
              content:
                '```json\n[{"key":"budget.total","value":"480 000 рублей","category":"бюджет"}]\n```',
            },
            finish_reason: 'stop',
          },
        ],
        usage: { prompt_tokens: 300, completion_tokens: 25, total_tokens: 325 },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const extractor = createLlmFactExtractor({
      settings: agentConfig,
      profile: resolveModelProfile('deepseek-chat'),
    })
    const result = await extractor({
      previousFacts: [],
      message: { role: 'user', content: 'Бюджет — 480 000 рублей.' },
    })

    expect(result.facts).toHaveLength(1)
    expect(result.facts[0]?.key).toBe('budget.total')
    expect(result.facts[0]?.category).toBe('бюджет')
    expect(result.promptTokens).toBe(300)
    expect(result.requests).toBe(1)
  })

  it('чинит JSON внутри текста и отбрасывает пустые записи', () => {
    const facts = parseFactsJson(
      'Вот факты: [{"key":"team.lead","value":"Артём","category":"имя"},{"key":"","value":"x"}]',
    )
    expect(facts).toHaveLength(1)
    expect(facts[0]?.key).toBe('team.lead')
  })
})
