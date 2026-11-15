import { buildEndpoint, createChatCompletion } from '../api/llm'
import {
  compressHistory,
  type CompressionConfig,
  type CompressionRunStats,
  type ContextSummary,
  type SummaryGenerator,
} from '../agent/compression'
import { repairToolCallSequence } from '../agent/history'
import {
  buildPromptEstimate,
  estimateCost,
  estimateMessagesTokens,
  estimateTextTokens,
  type ModelProfile,
} from '../tokens'
import type { ChatMessage, LlmExchange, LlmRequest, LlmSettings, TokenUsage } from '../types'
import { answerMentions, type MemoryDetail } from './dialog'

/**
 * Проверка памяти: один и тот же набор вопросов задаётся модели дважды —
 * с полной историей и с историей после сжатия. Ответ считается «вспомненным»,
 * если в нём есть ожидаемая деталь (имя, срок, сумма, решение).
 *
 * Это не магическая метрика качества: модель может ответить своими словами,
 * поэтому ключевые слова подобраны с запасом. Зато видно, какие детали
 * переживают сжатие, а какие исчезают.
 */

export const MEMORY_SYSTEM_PROMPT = `Ты — ассистент проекта. Отвечай на вопрос пользователя одной короткой фразой, опираясь только на историю диалога. Если нужной детали в истории нет, ответь «не знаю». Не выдумывай числа, имена и даты.`

export interface MemoryCheck {
  detailId: string
  question: string
  answer: string
  remembered: boolean
  promptTokens: number
  completionTokens: number
  costUsd: number
  error?: string
}

export interface MemoryModeResult {
  mode: 'full' | 'compressed'
  checks: MemoryCheck[]
  remembered: number
  total: number
  promptTokens: number
  completionTokens: number
  costUsd: number
  historyTokens: number
  summaryPromptTokens: number
  summaryCompletionTokens: number
  summaryCostUsd: number
  summaryRequests: number
}

export interface MemoryBenchmarkReport {
  startedAt: string
  finishedAt: string
  details: MemoryDetail[]
  full: MemoryModeResult
  compressed: MemoryModeResult
  summary: ContextSummary | null
  compression: CompressionRunStats
  /** Экономия только на prompt-токенах вопросов. */
  savedPromptTokens: number
  /** Экономия с учётом запросов на само сжатие. */
  netSavedPromptTokens: number
  savedCostUsd: number
  savedRatio: number
}

export interface RunMemoryBenchmarkInput {
  messages: ChatMessage[]
  details: MemoryDetail[]
  settings: LlmSettings
  profile: ModelProfile
  compression: CompressionConfig
  summary?: ContextSummary | null
  systemPrompt?: string
  generateSummary?: SummaryGenerator
  signal?: AbortSignal
  maxAnswerTokens?: number
  onProgress?: (text: string, done: number, total: number, mode: 'full' | 'compressed') => void
  onExchangeStart?: (exchange: LlmExchange) => void
  onExchangeEnd?: (exchange: LlmExchange) => void
}

function makeId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `id-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function contentToText(content: unknown): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map(part => {
        if (typeof part === 'string') return part
        if (part && typeof part === 'object' && 'text' in part) {
          const text = (part as { text?: unknown }).text
          if (typeof text === 'string') return text
        }
        return ''
      })
      .join('')
  }
  return ''
}

async function prepareBase(
  input: RunMemoryBenchmarkInput,
  mode: 'full' | 'compressed',
): Promise<{
  base: ChatMessage[]
  summary: ContextSummary | null
  compression: CompressionRunStats | null
}> {
  const history = repairToolCallSequence(
    input.messages.filter(message => message.role !== 'system'),
  )

  if (mode === 'full') {
    return { base: history, summary: null, compression: null }
  }

  const outcome = await compressHistory({
    messages: history,
    config: input.compression,
    summary: input.summary,
    generateSummary: input.generateSummary,
  })

  return {
    base: outcome.prepared,
    summary: outcome.summary,
    compression: outcome.stats,
  }
}

async function runMode(
  input: RunMemoryBenchmarkInput,
  mode: 'full' | 'compressed',
): Promise<{
  result: MemoryModeResult
  summary: ContextSummary | null
  compression: CompressionRunStats | null
}> {
  const prepared = await prepareBase(input, mode)
  const systemPrompt = input.systemPrompt ?? MEMORY_SYSTEM_PROMPT
  const maxAnswerTokens = input.maxAnswerTokens ?? 96

  const checks: MemoryCheck[] = []
  let promptTokensTotal = 0
  let completionTokensTotal = 0
  let costTotal = 0

  for (let index = 0; index < input.details.length; index += 1) {
    const detail = input.details[index]!
    if (input.signal?.aborted) throw new DOMException('Отменено', 'AbortError')

    input.onProgress?.(
      `${mode === 'full' ? 'Без сжатия' : 'Со сжатием'}: вопрос ${index + 1} из ${input.details.length}`,
      index,
      input.details.length,
      mode,
    )

    const request: LlmRequest = {
      model: input.settings.model,
      messages: [
        { role: 'system', content: systemPrompt },
        ...prepared.base,
        { role: 'user', content: detail.question },
      ],
      temperature: 0,
      max_tokens: maxAnswerTokens,
    }

    const exchange: LlmExchange = {
      id: makeId(),
      kind: 'chat',
      url: buildEndpoint(input.settings.baseUrl),
      startedAt: new Date().toISOString(),
      durationMs: 0,
      request,
      estimate: buildPromptEstimate(request.messages, [], input.profile.contextLimit),
    }
    input.onExchangeStart?.(exchange)
    const started = performance.now()

    let answer = ''
    let usage: TokenUsage | undefined
    let promptTokens = exchange.estimate?.totalTokens ?? 0
    let completionTokens = 0
    let costUsd = 0
    let error: string | undefined

    try {
      const response = await createChatCompletion(
        input.settings.baseUrl,
        input.settings.apiKey,
        request,
        input.signal,
      )
      exchange.durationMs = Math.round(performance.now() - started)
      exchange.response = response
      usage = response.usage
      exchange.usage = usage

      answer = contentToText(response.choices?.[0]?.message?.content).trim()
      promptTokens = usage?.prompt_tokens ?? exchange.estimate?.totalTokens ?? 0
      completionTokens = usage?.completion_tokens ?? estimateTextTokens(answer)
      costUsd = estimateCost(input.profile, promptTokens, completionTokens).totalUsd
      exchange.costUsd = costUsd
      exchange.message = { role: 'assistant', content: answer }
    } catch (caught) {
      exchange.durationMs = Math.round(performance.now() - started)
      error = caught instanceof Error ? caught.message : String(caught)
      exchange.error = error
      input.onExchangeEnd?.(exchange)
      throw caught
    }

    input.onExchangeEnd?.(exchange)
    const remembered = answerMentions(detail, answer)
    checks.push({
      detailId: detail.id,
      question: detail.question,
      answer,
      remembered,
      promptTokens,
      completionTokens,
      costUsd,
      ...(error ? { error } : {}),
    })
    promptTokensTotal += promptTokens
    completionTokensTotal += completionTokens
    costTotal += costUsd
  }

  input.onProgress?.(
    `${mode === 'full' ? 'Без сжатия' : 'Со сжатием'}: готово`,
    input.details.length,
    input.details.length,
    mode,
  )

  const compression = prepared.compression
  return {
    result: {
      mode,
      checks,
      remembered: checks.filter(check => check.remembered).length,
      total: checks.length,
      promptTokens: promptTokensTotal,
      completionTokens: completionTokensTotal,
      costUsd: costTotal,
      historyTokens: estimateMessagesTokens(prepared.base),
      summaryPromptTokens: compression?.summaryPromptTokens ?? 0,
      summaryCompletionTokens: compression?.summaryCompletionTokens ?? 0,
      summaryCostUsd: compression?.summaryCostUsd ?? 0,
      summaryRequests: compression?.summaryRequests ?? 0,
    },
    summary: prepared.summary,
    compression,
  }
}

export async function runMemoryBenchmark(
  input: RunMemoryBenchmarkInput,
): Promise<MemoryBenchmarkReport> {
  const startedAt = new Date().toISOString()
  const full = await runMode(input, 'full')
  const compressed = await runMode(input, 'compressed')
  const finishedAt = new Date().toISOString()

  const savedPromptTokens = full.result.promptTokens - compressed.result.promptTokens
  const netSavedPromptTokens =
    full.result.promptTokens -
    (compressed.result.promptTokens + compressed.result.summaryPromptTokens)
  const savedCostUsd =
    full.result.costUsd -
    (compressed.result.costUsd + compressed.result.summaryCostUsd)

  return {
    startedAt,
    finishedAt,
    details: input.details,
    full: full.result,
    compressed: compressed.result,
    summary: compressed.summary,
    compression:
      compressed.compression ?? {
        enabled: input.compression.enabled,
        usedSummary: false,
        summaryUpdated: false,
        summaryUpdates: 0,
        summaryRequests: 0,
        summaryPromptTokens: 0,
        summaryCompletionTokens: 0,
        summaryCostUsd: 0,
        summaryUsageReported: 0,
        fullHistoryTokens: full.result.historyTokens,
        sentHistoryTokens: compressed.result.historyTokens,
        summaryTokens: 0,
        tailTokens: compressed.result.historyTokens,
        savedTokens: full.result.historyTokens - compressed.result.historyTokens,
        recentMessages: 0,
        summarizedMessages: 0,
      },
    savedPromptTokens,
    netSavedPromptTokens,
    savedCostUsd,
    savedRatio:
      full.result.promptTokens > 0
        ? savedPromptTokens / full.result.promptTokens
        : 0,
  }
}
