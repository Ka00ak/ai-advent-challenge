import { afterEach, describe, expect, it, vi } from 'vitest'
import { Agent, type AgentConfig } from '../src/agent/Agent'
import {
  buildCompressedContext,
  compressHistory,
  findRecentStart,
  normalizeCompression,
  planCompression,
  previewCompression,
  summarizeLocally,
  type CompressionConfig,
} from '../src/agent/compression'
import { SUMMARY_SYSTEM_PROMPT, createLlmSummaryGenerator } from '../src/agent/summarizer'
import { ToolRegistry } from '../src/agent/ToolRegistry'
import { buildMemoryDialog, MEMORY_DETAILS, messagesContainDetail } from '../src/memory/dialog'
import { estimateMessagesTokens, resolveModelProfile } from '../src/tokens'
import { calculatorTool } from '../src/tools/calculator'
import type { ChatMessage, LlmExchange, LlmRequest } from '../src/types'

const localConfig: CompressionConfig = {
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
  compression: localConfig,
  summary: null,
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

describe('план сжатия', () => {
  it('оставляет последние N сообщений и выравнивает окно по границе пользователя', () => {
    const dialog = buildMemoryDialog()
    const start = findRecentStart(dialog, 8)

    expect(dialog.length).toBe(24)
    expect(start).toBe(16)
    expect(dialog[start]?.role).toBe('user')
  })

  it('не разрывает пару assistant(tool_calls) → tool', () => {
    const messages: ChatMessage[] = [
      { role: 'user', content: 'Посчитай' },
      {
        role: 'assistant',
        content: null,
        tool_calls: [
          {
            id: 'call_1',
            type: 'function',
            function: { name: 'calculate', arguments: '{"expression":"2+2"}' },
          },
        ],
      },
      { role: 'tool', tool_call_id: 'call_1', name: 'calculate', content: '{"ok":true}' },
      { role: 'assistant', content: 'Готово.' },
      { role: 'user', content: 'Спасибо' },
      { role: 'assistant', content: 'Пожалуйста.' },
    ]

    expect(findRecentStart(messages, 1)).toBe(4)
    expect(findRecentStart(messages, 3)).toBe(0)
  })

  it('не сжимает, пока старых сообщений меньше порога', () => {
    const dialog = buildMemoryDialog().slice(0, 14)
    const plan = planCompression(dialog, localConfig, null)

    expect(plan.needsSummary).toBe(false)
    expect(plan.summarizeTo).toBe(0)
  })

  it('при накоплении порога указывает границы порции', () => {
    const dialog = buildMemoryDialog()
    const plan = planCompression(dialog, localConfig, null)

    expect(plan.needsSummary).toBe(true)
    expect(plan.summarizeFrom).toBe(0)
    expect(plan.summarizeTo).toBe(16)
    expect(plan.pendingMessages).toBe(16)
    expect(plan.recentMessages).toBe(8)
  })

  it('выключенное сжатие не меняет историю', () => {
    const dialog = buildMemoryDialog()
    const config = normalizeCompression({
      ...localConfig,
      enabled: false,
    })
    const prepared = buildCompressedContext(dialog, config, null)

    expect(prepared).toHaveLength(dialog.length)
    expect(estimateMessagesTokens(prepared)).toBe(estimateMessagesTokens(dialog))
  })
})

describe('локальное сжатие', () => {
  it('заменяет старую часть сводкой и экономит токены', async () => {
    const dialog = buildMemoryDialog()
    const outcome = await compressHistory({
      messages: dialog,
      config: localConfig,
      summary: null,
      now: () => new Date('2026-11-15T12:00:00.000Z'),
    })

    expect(outcome.stats.summaryUpdated).toBe(true)
    expect(outcome.stats.usedSummary).toBe(true)
    expect(outcome.stats.summarizedMessages).toBe(16)
    expect(outcome.stats.recentMessages).toBe(8)
    expect(outcome.summary?.coveredMessages).toBe(16)
    expect(outcome.summary?.mode).toBe('local')
    expect(outcome.prepared).toHaveLength(9) // system-сводка + 8 последних
    expect(outcome.stats.sentHistoryTokens).toBeLessThan(outcome.stats.fullHistoryTokens)
    expect(outcome.stats.savedTokens).toBeGreaterThan(0)
  })

  it('локальная сводка сохраняет все 12 контрольных деталей', async () => {
    const dialog = buildMemoryDialog()
    const outcome = await compressHistory({
      messages: dialog,
      config: localConfig,
      summary: null,
    })

    for (const detail of MEMORY_DETAILS) {
      expect(messagesContainDetail(outcome.prepared, detail)).toBe(true)
    }
  })

  it('сохраняет все сообщения пользователя и сокращает ответы ассистента', () => {
    const dialog = buildMemoryDialog().slice(0, 6)
    const summary = summarizeLocally(dialog)
    const userTexts = dialog.filter(message => message.role === 'user')

    expect(summary.length).toBeGreaterThan(0)
    for (const message of userTexts) {
      expect(summary).toContain(message.content?.slice(0, 40) ?? '')
    }
  })

  it('показывает метрики до и после без вызова API', () => {
    const dialog = buildMemoryDialog()
    const preview = previewCompression(dialog, localConfig, null)

    expect(preview.fullHistoryTokens).toBeGreaterThan(0)
    expect(preview.sentHistoryTokens).toBe(preview.fullHistoryTokens)
    expect(preview.pendingMessages).toBe(16)
    expect(preview.needsSummary).toBe(true)
  })

  it('использует переданный генератор сводки и считает его токены', async () => {
    const dialog = buildMemoryDialog()
    let calls = 0
    const outcome = await compressHistory({
      messages: dialog,
      config: { ...localConfig, mode: 'llm' },
      summary: null,
      generateSummary: async input => {
        calls += 1
        expect(input.messages).toHaveLength(16)
        return {
          text: 'LLM-сводка: все детали сохранены.',
          mode: 'llm',
          requests: 1,
          promptTokens: 900,
          completionTokens: 80,
          costUsd: 0.0003,
          usageReported: true,
        }
      },
    })

    expect(calls).toBe(1)
    expect(outcome.summary?.mode).toBe('llm')
    expect(outcome.summary?.text).toContain('LLM-сводка')
    expect(outcome.stats.summaryRequests).toBe(1)
    expect(outcome.stats.summaryPromptTokens).toBe(900)
    expect(outcome.stats.summaryCompletionTokens).toBe(80)
    expect(outcome.stats.summaryCostUsd).toBeCloseTo(0.0003, 8)
  })

  it('откатывается на локальную сводку, если LLM-генератор упал', async () => {
    const dialog = buildMemoryDialog()
    const outcome = await compressHistory({
      messages: dialog,
      config: { ...localConfig, mode: 'llm' },
      summary: null,
      generateSummary: async () => {
        throw new Error('API недоступен')
      },
    })

    expect(outcome.summary?.mode).toBe('local')
    expect(outcome.stats.summaryError).toContain('API недоступен')
    expect(outcome.stats.usedSummary).toBe(true)
  })
})

describe('LLM-сводка', () => {
  it('отправляет отдельный summary-запрос и возвращает usage', async () => {
    const requests: LlmRequest[] = []
    const exchanges: LlmExchange[] = []
    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      requests.push(JSON.parse(String(init?.body)) as LlmRequest)
      return jsonResponse({
        id: 'resp_summary',
        choices: [
          {
            index: 0,
            message: {
              role: 'assistant',
              content: '- Проект «Атлас», дедлайн 15 декабря 2026.',
            },
            finish_reason: 'stop',
          },
        ],
        usage: { prompt_tokens: 500, completion_tokens: 40, total_tokens: 540 },
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const generator = createLlmSummaryGenerator({
      settings: agentConfig,
      profile: resolveModelProfile('deepseek-chat'),
      onExchangeStart: exchange => exchanges.push(exchange),
      onExchangeEnd: exchange => {
        const index = exchanges.findIndex(item => item.id === exchange.id)
        if (index >= 0) exchanges.splice(index, 1, exchange)
      },
    })

    const result = await generator({
      previousSummary: '- Проект «Атлас»',
      messages: buildMemoryDialog().slice(0, 4),
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(requests[0]?.messages[0]?.content).toBe(SUMMARY_SYSTEM_PROMPT)
    expect(requests[0]?.messages[1]?.content).toContain('Предыдущая сводка')
    expect(result.mode).toBe('llm')
    expect(result.requests).toBe(1)
    expect(result.promptTokens).toBe(500)
    expect(result.completionTokens).toBe(40)
    expect(result.usageReported).toBe(true)
    expect(exchanges[0]?.kind).toBe('summary')
    expect(exchanges[0]?.usage?.prompt_tokens).toBe(500)
    expect(exchanges[0]?.costUsd).toBeGreaterThan(0)
  })

  it('Agent сам вызывает LLM-сжатие перед основным запросом', async () => {
    const requests: LlmRequest[] = []
    const exchanges: LlmExchange[] = []
    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      const request = JSON.parse(String(init?.body)) as LlmRequest
      requests.push(request)
      if (request.messages[0]?.content === SUMMARY_SYSTEM_PROMPT) {
        return jsonResponse({
          choices: [
            {
              index: 0,
              message: {
                role: 'assistant',
                content: '- Проект «Атлас». Бюджет 480 000 рублей. Дедлайн 15 декабря 2026.',
              },
              finish_reason: 'stop',
            },
          ],
          usage: { prompt_tokens: 400, completion_tokens: 30, total_tokens: 430 },
        })
      }
      return jsonResponse(completion('Бюджет — 480 000 рублей.'))
    })
    vi.stubGlobal('fetch', fetchMock)

    const agent = new Agent(
      {
        ...agentConfig,
        compression: { ...localConfig, mode: 'llm' },
      },
      new ToolRegistry([calculatorTool]),
    )
    const result = await agent.run('Какой бюджет у проекта?', buildMemoryDialog(), {
      onExchangeStart: exchange => exchanges.push(exchange),
    })

    expect(requests).toHaveLength(2)
    expect(result.summary?.mode).toBe('llm')
    expect(result.compression.summaryRequests).toBe(1)
    expect(result.compression.summaryPromptTokens).toBe(400)
    expect(result.compression.summaryCompletionTokens).toBe(30)
    expect(exchanges.map(exchange => exchange.kind)).toEqual(['summary', 'chat'])
    expect(
      requests[1]?.messages.some(message =>
        message.content?.includes('Сводка предыдущей части диалога'),
      ),
    ).toBe(true)
  })
})

describe('Agent со сжатием', () => {
  it('отправляет summary вместо старых сообщений, но сохраняет полную историю', async () => {
    const requests: LlmRequest[] = []
    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      requests.push(JSON.parse(String(init?.body)) as LlmRequest)
      return jsonResponse(completion('Короткий ответ.'))
    })
    vi.stubGlobal('fetch', fetchMock)

    const dialog = buildMemoryDialog()
    const agent = new Agent(agentConfig, new ToolRegistry([calculatorTool]))
    const result = await agent.run('Какой бюджет у проекта?', dialog)

    expect(result.stoppedReason).toBe('final')
    expect(result.compression.usedSummary).toBe(true)
    expect(result.compression.summarizedMessages).toBe(16)
    expect(result.summary?.coveredMessages).toBe(16)

    const sent = requests[0]?.messages ?? []
    expect(sent[0]).toMatchObject({ role: 'system' })
    expect(sent[1]?.role).toBe('system')
    expect(sent[1]?.content).toContain('Сводка предыдущей части диалога')
    expect(sent.length).toBeLessThan(dialog.length)
    expect(sent.at(-1)).toMatchObject({ role: 'user', content: 'Какой бюджет у проекта?' })

    // Каноническая история не потеряла ни одного сообщения.
    expect(result.messages).toHaveLength(dialog.length + 2)
    expect(result.messages.filter(message => message.role === 'user')).toHaveLength(13)
  })

  it('не обновляет сводку повторно, если новых старых сообщений нет', async () => {
    const dialog = buildMemoryDialog()
    const first = await compressHistory({ messages: dialog, config: localConfig })
    const second = await compressHistory({
      messages: dialog,
      config: localConfig,
      summary: first.summary,
    })

    expect(second.stats.summaryUpdated).toBe(false)
    expect(second.summary?.coveredMessages).toBe(16)
    expect(second.prepared).toHaveLength(9)
  })

  it('добавляет новые старые сообщения к существующей сводке', async () => {
    const dialog = buildMemoryDialog()
    const first = await compressHistory({ messages: dialog, config: localConfig })
    const additions: ChatMessage[] = []
    for (let index = 0; index < 5; index += 1) {
      additions.push({
        role: 'user',
        content:
          index === 0
            ? 'Новая вводная: бюджет увеличен до 500 000 рублей.'
            : `Организационная реплика ${index + 1} без новых деталей.`,
      })
      additions.push({ role: 'assistant', content: 'Записал.' })
    }
    const extended: ChatMessage[] = [...dialog, ...additions]

    const second = await compressHistory({
      messages: extended,
      config: localConfig,
      summary: first.summary,
    })

    // Сверх последних 8 сообщений накопилось ровно 10 старых — пора обновлять.
    expect(second.stats.summaryUpdated).toBe(true)
    expect(second.summary!.coveredMessages).toBeGreaterThan(16)
    expect(second.summary?.chunks).toBe(2)
    expect(second.summary?.text).toContain('500 000')
  })
})
