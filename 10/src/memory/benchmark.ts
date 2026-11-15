import { buildEndpoint, createChatCompletion } from '../api/llm'
import type {
  CompressionConfig,
  CompressionRunStats,
  ContextSummary,
  SummaryGenerator,
} from '../agent/compression'
import {
  normalizeContextConfig,
  prepareContext,
  type ContextRunStats,
  type ContextStrategy,
} from '../agent/context'
import type { FactExtractor, FactMode, StickyFactsState } from '../agent/facts'
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
 * Проверка памяти: один и тот же набор вопросов задаётся модели в нескольких
 * режимах. Ответ считается «вспомненным», если в нём есть ожидаемая деталь.
 *
 * Day 10 добавляет к full/summary режимы sliding и facts. Все прогоны идут на
 * одном диалоге и одном списке деталей, поэтому таблица сравнения честная:
 * отличается только то, что реально уходит в запрос.
 */

export const MEMORY_SYSTEM_PROMPT = `Ты — ассистент проекта. Отвечай на вопрос пользователя одной короткой фразой, опираясь только на историю диалога и блок памяти. Если нужной детали в истории нет, ответь «не знаю». Не выдумывай числа, имена и даты.`

export type BenchmarkMode = 'full' | 'sliding' | 'facts' | 'summary'

export const ALL_BENCHMARK_MODES: BenchmarkMode[] = [
  'full',
  'sliding',
  'facts',
  'summary',
]

export const BENCHMARK_MODE_LABELS: Record<BenchmarkMode, string> = {
  full: 'Без стратегии',
  sliding: 'Sliding Window',
  facts: 'Sticky Facts',
  summary: 'Summary',
}

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
  mode: BenchmarkMode
  label: string
  checks: MemoryCheck[]
  remembered: number
  total: number
  promptTokens: number
  completionTokens: number
  costUsd: number
  /** Оценка того, что реально уходит в запрос, без system-промпта. */
  historyTokens: number
  /** Те же токены под явным именем для сравнительной таблицы. */
  contextTokens: number
  /** Токены на обновление памяти (summary или facts), если они были. */
  extractionRequests: number
  extractionPromptTokens: number
  extractionCompletionTokens: number
  extractionCostUsd: number
  /** prompt всех вопросов + prompt на обновление памяти. */
  netPromptTokens: number
  factsCount: number
  summaryPromptTokens: number
  summaryCompletionTokens: number
  summaryCostUsd: number
  summaryRequests: number
}

export interface MemoryBenchmarkReport {
  startedAt: string
  finishedAt: string
  details: MemoryDetail[]
  modes: BenchmarkMode[]
  full: MemoryModeResult
  /** Summary-режим: сохранено имя для совместимости с day 09. */
  compressed: MemoryModeResult
  sliding: MemoryModeResult | null
  facts: MemoryModeResult | null
  strategies: MemoryModeResult[]
  summary: ContextSummary | null
  factsState: StickyFactsState | null
  compression: CompressionRunStats
  /** Экономия только на prompt-токенах вопросов (summary против full). */
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
  facts?: StickyFactsState | null
  factsMode?: FactMode
  factExtractor?: FactExtractor
  systemPrompt?: string
  generateSummary?: SummaryGenerator
  signal?: AbortSignal
  maxAnswerTokens?: number
  modes?: BenchmarkMode[]
  onProgress?: (text: string, done: number, total: number, mode: BenchmarkMode) => void
  onExchangeStart?: (exchange: LlmExchange) => void
  onExchangeEnd?: (exchange: LlmExchange) => void
}

interface PreparedBase {
  base: ChatMessage[]
  summary: ContextSummary | null
  facts: StickyFactsState | null
  compression: CompressionRunStats | null
  context: ContextRunStats | null
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

function emptyCompressionStats(
  fullHistoryTokens: number,
  sentHistoryTokens: number,
): CompressionRunStats {
  return {
    enabled: false,
    usedSummary: false,
    summaryUpdated: false,
    summaryUpdates: 0,
    summaryRequests: 0,
    summaryPromptTokens: 0,
    summaryCompletionTokens: 0,
    summaryCostUsd: 0,
    summaryUsageReported: 0,
    fullHistoryTokens,
    sentHistoryTokens,
    summaryTokens: 0,
    tailTokens: sentHistoryTokens,
    savedTokens: fullHistoryTokens - sentHistoryTokens,
    recentMessages: 0,
    summarizedMessages: 0,
  }
}

function modeToStrategy(mode: BenchmarkMode): ContextStrategy {
  if (mode === 'summary') return 'summary'
  if (mode === 'facts') return 'facts'
  return 'sliding'
}

async function prepareBase(
  input: RunMemoryBenchmarkInput,
  mode: BenchmarkMode,
): Promise<PreparedBase> {
  const history = repairToolCallSequence(
    input.messages.filter(message => message.role !== 'system'),
  )

  if (mode === 'full') {
    return {
      base: history,
      summary: null,
      facts: null,
      compression: null,
      context: null,
    }
  }

  const config = normalizeContextConfig({
    strategy: modeToStrategy(mode),
    keepRecent: input.compression?.keepRecent ?? 8,
    compression: input.compression,
    factsMode: input.factsMode ?? 'local',
  })

  const outcome = await prepareContext({
    messages: history,
    config,
    summary: mode === 'summary' ? input.summary : null,
    // Facts всегда строим из проверяемого диалога, а не из чужой ветки:
    // иначе сравнение стратегий будет нечестным.
    facts: mode === 'facts' ? (input.facts ?? null) : null,
    generateSummary: mode === 'summary' ? input.generateSummary : undefined,
    extractFacts: mode === 'facts' ? input.factExtractor : undefined,
  })

  return {
    base: outcome.prepared,
    summary: outcome.summary,
    facts: outcome.facts,
    compression: outcome.compression,
    context: outcome.stats,
  }
}

async function runMode(
  input: RunMemoryBenchmarkInput,
  mode: BenchmarkMode,
): Promise<{ result: MemoryModeResult; prepared: PreparedBase }> {
  const prepared = await prepareBase(input, mode)
  const systemPrompt = input.systemPrompt ?? MEMORY_SYSTEM_PROMPT
  const maxAnswerTokens = input.maxAnswerTokens ?? 96
  const label = BENCHMARK_MODE_LABELS[mode]

  const checks: MemoryCheck[] = []
  let promptTokensTotal = 0
  let completionTokensTotal = 0
  let costTotal = 0

  for (let index = 0; index < input.details.length; index += 1) {
    const detail = input.details[index]!
    if (input.signal?.aborted) throw new DOMException('Отменено', 'AbortError')

    input.onProgress?.(
      `${label}: вопрос ${index + 1} из ${input.details.length}`,
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
    `${label}: готово`,
    input.details.length,
    input.details.length,
    mode,
  )

  const context = prepared.context
  const historyTokens = estimateMessagesTokens(prepared.base)
  const extractionRequests =
    mode === 'summary'
      ? (context?.summaryRequests ?? 0)
      : mode === 'facts'
        ? (context?.factsRequests ?? 0)
        : 0
  const extractionPromptTokens =
    mode === 'summary'
      ? (context?.summaryPromptTokens ?? 0)
      : mode === 'facts'
        ? (context?.factsPromptTokens ?? 0)
        : 0
  const extractionCompletionTokens =
    mode === 'summary'
      ? (context?.summaryCompletionTokens ?? 0)
      : mode === 'facts'
        ? (context?.factsCompletionTokens ?? 0)
        : 0
  const extractionCostUsd =
    mode === 'summary'
      ? (context?.summaryCostUsd ?? 0)
      : mode === 'facts'
        ? (context?.factsCostUsd ?? 0)
        : 0

  return {
    prepared,
    result: {
      mode,
      label,
      checks,
      remembered: checks.filter(check => check.remembered).length,
      total: checks.length,
      promptTokens: promptTokensTotal,
      completionTokens: completionTokensTotal,
      costUsd: costTotal,
      historyTokens,
      contextTokens: historyTokens,
      extractionRequests,
      extractionPromptTokens,
      extractionCompletionTokens,
      extractionCostUsd,
      netPromptTokens: promptTokensTotal + extractionPromptTokens,
      factsCount: context?.factsCount ?? 0,
      summaryPromptTokens: context?.summaryPromptTokens ?? 0,
      summaryCompletionTokens: context?.summaryCompletionTokens ?? 0,
      summaryCostUsd: context?.summaryCostUsd ?? 0,
      summaryRequests: context?.summaryRequests ?? 0,
    },
  }
}

export async function runMemoryBenchmark(
  input: RunMemoryBenchmarkInput,
): Promise<MemoryBenchmarkReport> {
  const startedAt = new Date().toISOString()
  const requested: BenchmarkMode[] = input.modes?.length
    ? [...input.modes]
    : ['full', 'summary']
  const modes: BenchmarkMode[] = requested.includes('full')
    ? requested
    : ['full', ...requested]

  const byMode = new Map<BenchmarkMode, { result: MemoryModeResult; prepared: PreparedBase }>()
  for (const mode of modes) {
    if (byMode.has(mode)) continue
    byMode.set(mode, await runMode(input, mode))
  }

  const fullEntry = byMode.get('full')!
  const summaryEntry = byMode.get('summary')
  const factsEntry = byMode.get('facts')
  const slidingEntry = byMode.get('sliding')

  const full = fullEntry.result
  const compressed = summaryEntry?.result ?? full
  const strategies = modes
    .map(mode => byMode.get(mode)?.result)
    .filter((result): result is MemoryModeResult => Boolean(result))

  const finishedAt = new Date().toISOString()

  const summaryExtractionPrompt = compressed.extractionPromptTokens
  const savedPromptTokens = full.promptTokens - compressed.promptTokens
  const netSavedPromptTokens =
    full.promptTokens - (compressed.promptTokens + summaryExtractionPrompt)
  const savedCostUsd =
    full.costUsd - (compressed.costUsd + compressed.extractionCostUsd)

  const summaryCompression =
    summaryEntry?.prepared.compression ??
    emptyCompressionStats(full.historyTokens, summaryEntry?.result.historyTokens ?? full.historyTokens)

  return {
    startedAt,
    finishedAt,
    details: input.details,
    modes,
    full,
    compressed,
    sliding: slidingEntry?.result ?? null,
    facts: factsEntry?.result ?? null,
    strategies,
    summary: summaryEntry?.prepared.summary ?? null,
    factsState: factsEntry?.prepared.facts ?? null,
    compression: summaryCompression,
    savedPromptTokens,
    netSavedPromptTokens,
    savedCostUsd,
    savedRatio:
      full.promptTokens > 0
        ? savedPromptTokens / full.promptTokens
        : 0,
  }
}
