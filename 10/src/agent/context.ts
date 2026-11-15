import { estimateMessageTokens, estimateMessagesTokens } from '../tokens'
import type { ChatMessage } from '../types'
import {
  buildCompressedContext,
  compressHistory,
  findRecentStart,
  normalizeCompression,
  previewCompression,
  type CompressionConfig,
  type CompressionRunStats,
  type ContextSummary,
  type SummaryGenerator,
} from './compression'
import {
  factsMessage,
  normalizeFactsState,
  updateFacts,
  type FactExtractor,
  type FactMode,
  type FactsUpdateStats,
  type StickyFactsState,
} from './facts'

/**
 * Три стратегии управления контекстом.
 *
 * summary  — старые сообщения сворачиваются в сводку, в запрос уходит
 *            summary + последние N сообщений;
 * sliding  — в запрос уходят только последние N сообщений, остальное
 *            отбрасывается безвозвратно;
 * facts    — отдельная key-value память обновляется после каждого сообщения
 *            пользователя, в запрос уходят facts + последние N сообщений.
 */

export type ContextStrategy = 'summary' | 'sliding' | 'facts'

export interface ContextStrategyMeta {
  id: ContextStrategy
  label: string
  short: string
  description: string
  extraRequest: string
  difficulty: 'низкая' | 'средняя' | 'высокая'
  bestFor: string
  loses: string
}

export const CONTEXT_STRATEGIES: ContextStrategyMeta[] = [
  {
    id: 'sliding',
    label: 'Sliding Window',
    short: 'последние N',
    description:
      'В запрос уходят только последние N сообщений. Всё, что старше окна, отбрасывается.',
    extraRequest: '0 запросов к модели',
    difficulty: 'низкая',
    bestFor: 'короткие сессии, поддержка, где важна только свежая переписка',
    loses: 'все факты старше окна исчезают полностью',
  },
  {
    id: 'facts',
    label: 'Sticky Facts',
    short: 'facts + последние N',
    description:
      'Отдельная key-value память: цель, имена, сроки, бюджеты, ограничения, решения и договорённости. Обновляется после каждой реплики пользователя.',
    extraRequest: '1 запрос на сообщение (LLM) или 0 (local)',
    difficulty: 'средняя',
    bestFor: 'долгие проектные диалоги, где важны конкретные детали',
    loses: 'нюансы и контекст, которые не попали в ключ-значение',
  },
  {
    id: 'summary',
    label: 'Summary',
    short: 'сводка + последние N',
    description:
      'Старая часть истории сжимается в связную сводку порциями. В запрос уходят сводка и последние N сообщений.',
    extraRequest: '1 запрос на порцию старых сообщений',
    difficulty: 'высокая',
    bestFor: 'длинные свободные диалоги, где важен общий смысл, а не таблица фактов',
    loses: 'часть чисел, имён и формулировок при пересказе',
  },
]

export function contextStrategyMeta(strategy: ContextStrategy): ContextStrategyMeta {
  return CONTEXT_STRATEGIES.find(item => item.id === strategy) ?? CONTEXT_STRATEGIES[2]!
}

export function contextStrategyLabel(strategy: ContextStrategy): string {
  return contextStrategyMeta(strategy).label
}

export interface ContextConfig {
  strategy: ContextStrategy
  /** Размер свежего окна для всех трёх стратегий. */
  keepRecent: number
  /** Параметры summary: порог, режим, включённость. */
  compression: CompressionConfig
  factsMode: FactMode
  maxFacts: number
}

export interface ContextConfigInput {
  strategy?: ContextStrategy
  keepRecent?: number
  compression?: CompressionConfig | null
  factsMode?: FactMode
  maxFacts?: number
}

function clampInt(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min
  return Math.min(max, Math.max(min, Math.floor(value)))
}

export function normalizeContextConfig(input?: ContextConfigInput | null): ContextConfig {
  const compression = normalizeCompression(input?.compression)
  return {
    strategy:
      input?.strategy === 'sliding' || input?.strategy === 'facts'
        ? input.strategy
        : 'summary',
    keepRecent: clampInt(
      input?.keepRecent ?? compression.keepRecent,
      1,
      200,
    ),
    compression,
    factsMode: input?.factsMode === 'llm' ? 'llm' : 'local',
    maxFacts: clampInt(input?.maxFacts ?? 80, 1, 200),
  }
}

export interface ContextRunStats {
  strategy: ContextStrategy
  fullHistoryTokens: number
  sentHistoryTokens: number
  tailTokens: number
  summaryTokens: number
  factsTokens: number
  savedTokens: number
  savedRatio: number
  recentMessages: number
  droppedMessages: number
  summarizedMessages: number
  usedSummary: boolean
  usedFacts: boolean
  factsCount: number
  factsUpdated: boolean
  factsRequests: number
  factsPromptTokens: number
  factsCompletionTokens: number
  factsCostUsd: number
  factsError?: string
  summaryRequests: number
  summaryPromptTokens: number
  summaryCompletionTokens: number
  summaryCostUsd: number
  summaryUpdated: boolean
}

export interface ContextOutcome {
  strategy: ContextStrategy
  /** История для модели без основного system-промпта. */
  prepared: ChatMessage[]
  summary: ContextSummary | null
  facts: StickyFactsState | null
  compression: CompressionRunStats
  stats: ContextRunStats
  factsStats: FactsUpdateStats | null
}

function emptyCompressionStats(
  fullHistoryTokens: number,
  sentHistoryTokens: number,
  recentMessages: number,
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
    recentMessages,
    summarizedMessages: 0,
  }
}

export interface PrepareContextInput {
  messages: ChatMessage[]
  config: ContextConfig
  summary?: ContextSummary | null
  facts?: StickyFactsState | null
  generateSummary?: SummaryGenerator
  extractFacts?: FactExtractor
  now?: () => Date
  onFactsExtractionStart?: (message: ChatMessage) => void
  onFactsExtractionEnd?: (message: ChatMessage, error?: Error) => void
}

/**
 * Основная точка входа: обновляет память (если нужно) и собирает контекст под
 * выбранную стратегию. Полная история в результате не теряется — её сохраняет
 * вызывающая сторона.
 */
export async function prepareContext(input: PrepareContextInput): Promise<ContextOutcome> {
  const history = input.messages.filter(message => message.role !== 'system')
  const config = input.config
  const fullHistoryTokens = estimateMessagesTokens(history)

  if (config.strategy === 'summary') {
    const outcome = await compressHistory({
      messages: history,
      config: config.compression,
      summary: input.summary,
      generateSummary: input.generateSummary,
      now: input.now,
    })
    const summaryTokens = outcome.stats.summaryTokens
    const tailTokens = Math.max(0, outcome.stats.sentHistoryTokens - summaryTokens)
    const stats: ContextRunStats = {
      strategy: 'summary',
      fullHistoryTokens: outcome.stats.fullHistoryTokens,
      sentHistoryTokens: outcome.stats.sentHistoryTokens,
      tailTokens,
      summaryTokens,
      factsTokens: 0,
      savedTokens: outcome.stats.fullHistoryTokens - outcome.stats.sentHistoryTokens,
      savedRatio:
        outcome.stats.fullHistoryTokens > 0
          ? (outcome.stats.fullHistoryTokens - outcome.stats.sentHistoryTokens) /
            outcome.stats.fullHistoryTokens
          : 0,
      recentMessages: outcome.stats.recentMessages,
      droppedMessages: outcome.stats.summarizedMessages,
      summarizedMessages: outcome.stats.summarizedMessages,
      usedSummary: outcome.stats.usedSummary,
      usedFacts: false,
      factsCount: 0,
      factsUpdated: false,
      factsRequests: 0,
      factsPromptTokens: 0,
      factsCompletionTokens: 0,
      factsCostUsd: 0,
      summaryRequests: outcome.stats.summaryRequests,
      summaryPromptTokens: outcome.stats.summaryPromptTokens,
      summaryCompletionTokens: outcome.stats.summaryCompletionTokens,
      summaryCostUsd: outcome.stats.summaryCostUsd,
      summaryUpdated: outcome.stats.summaryUpdated,
    }
    return {
      strategy: 'summary',
      prepared: outcome.prepared,
      summary: outcome.summary,
      facts: input.facts ?? null,
      compression: outcome.stats,
      stats,
      factsStats: null,
    }
  }

  if (config.strategy === 'sliding') {
    const start = findRecentStart(history, config.keepRecent)
    const prepared = history.slice(start)
    const sentHistoryTokens = estimateMessagesTokens(prepared)
    return {
      strategy: 'sliding',
      prepared,
      summary: null,
      facts: input.facts ?? null,
      compression: emptyCompressionStats(fullHistoryTokens, sentHistoryTokens, history.length - start),
      stats: {
        strategy: 'sliding',
        fullHistoryTokens,
        sentHistoryTokens,
        tailTokens: sentHistoryTokens,
        summaryTokens: 0,
        factsTokens: 0,
        savedTokens: fullHistoryTokens - sentHistoryTokens,
        savedRatio:
          fullHistoryTokens > 0
            ? (fullHistoryTokens - sentHistoryTokens) / fullHistoryTokens
            : 0,
        recentMessages: history.length - start,
        droppedMessages: start,
        summarizedMessages: 0,
        usedSummary: false,
        usedFacts: false,
        factsCount: 0,
        factsUpdated: false,
        factsRequests: 0,
        factsPromptTokens: 0,
        factsCompletionTokens: 0,
        factsCostUsd: 0,
        summaryRequests: 0,
        summaryPromptTokens: 0,
        summaryCompletionTokens: 0,
        summaryCostUsd: 0,
        summaryUpdated: false,
      },
      factsStats: null,
    }
  }

  // Sticky Facts: сначала обновляем память после каждого нового сообщения
  // пользователя, затем отправляем facts + последние N сообщений.
  const update = await updateFacts(
    {
      messages: history,
      previous: input.facts,
      mode: config.factsMode,
      extractor: input.extractFacts,
      maxFacts: config.maxFacts,
      now: input.now,
    },
    {
      onExtractionStart: input.onFactsExtractionStart,
      onExtractionEnd: input.onFactsExtractionEnd,
    },
  )

  const facts = normalizeFactsState(update.facts, history.length)
  const start = findRecentStart(history, config.keepRecent)
  const tail = history.slice(start)
  const prepared: ChatMessage[] = []
  const factsTokens = facts?.facts.length ? estimateMessageTokens(factsMessage(facts)) : 0
  if (facts?.facts.length) prepared.push(factsMessage(facts))
  prepared.push(...tail)
  const sentHistoryTokens = estimateMessagesTokens(prepared)
  const tailTokens = Math.max(0, sentHistoryTokens - factsTokens)
  const stats: ContextRunStats = {
    strategy: 'facts',
    fullHistoryTokens,
    sentHistoryTokens,
    tailTokens,
    summaryTokens: 0,
    factsTokens,
    savedTokens: fullHistoryTokens - sentHistoryTokens,
    savedRatio:
      fullHistoryTokens > 0
        ? (fullHistoryTokens - sentHistoryTokens) / fullHistoryTokens
        : 0,
    recentMessages: history.length - start,
    droppedMessages: start,
    summarizedMessages: 0,
    usedSummary: false,
    usedFacts: Boolean(facts?.facts.length),
    factsCount: facts?.facts.length ?? 0,
    factsUpdated: update.stats.updatedMessages > 0,
    factsRequests: update.stats.requests,
    factsPromptTokens: update.stats.promptTokens,
    factsCompletionTokens: update.stats.completionTokens,
    factsCostUsd: update.stats.costUsd,
    ...(update.stats.error ? { factsError: update.stats.error } : {}),
    summaryRequests: 0,
    summaryPromptTokens: 0,
    summaryCompletionTokens: 0,
    summaryCostUsd: 0,
    summaryUpdated: false,
  }

  return {
    strategy: 'facts',
    prepared,
    summary: null,
    facts,
    compression: emptyCompressionStats(
      fullHistoryTokens,
      sentHistoryTokens,
      history.length - start,
    ),
    stats,
    factsStats: update.stats,
  }
}

/* ------------------------------------------------------------------ */
/* Синхронный предпросмотр (без обновления памяти и без API)            */
/* ------------------------------------------------------------------ */

export interface ContextPreview {
  strategy: ContextStrategy
  fullHistoryTokens: number
  sentHistoryTokens: number
  tailTokens: number
  summaryTokens: number
  factsTokens: number
  savedTokens: number
  savedRatio: number
  recentMessages: number
  droppedMessages: number
  summarizedMessages: number
  usedSummary: boolean
  usedFacts: boolean
  factsCount: number
  keepRecent: number
}

export function buildContextMessages(
  messages: ChatMessage[],
  config: ContextConfig,
  summary?: ContextSummary | null,
  facts?: StickyFactsState | null,
): ChatMessage[] {
  const history = messages.filter(message => message.role !== 'system')

  if (config.strategy === 'summary') {
    return buildCompressedContext(history, config.compression, summary)
  }

  const start = findRecentStart(history, config.keepRecent)
  const tail = history.slice(start)

  if (config.strategy === 'sliding') return [...tail]

  const normalizedFacts = normalizeFactsState(facts, history.length)
  const prepared: ChatMessage[] = []
  if (normalizedFacts?.facts.length) prepared.push(factsMessage(normalizedFacts))
  prepared.push(...tail)
  return prepared
}

export function previewContext(
  messages: ChatMessage[],
  config: ContextConfig,
  summary?: ContextSummary | null,
  facts?: StickyFactsState | null,
): ContextPreview {
  const history = messages.filter(message => message.role !== 'system')
  const fullHistoryTokens = estimateMessagesTokens(history)
  const recentStart = findRecentStart(history, config.keepRecent)

  if (config.strategy === 'summary') {
    const preview = previewCompression(history, config.compression, summary)
    return {
      strategy: 'summary',
      fullHistoryTokens: preview.fullHistoryTokens,
      sentHistoryTokens: preview.sentHistoryTokens,
      tailTokens: preview.tailTokens,
      summaryTokens: preview.summaryTokens,
      factsTokens: 0,
      savedTokens: preview.savedTokens,
      savedRatio: preview.savedRatio,
      recentMessages: preview.recentMessages,
      droppedMessages: preview.summarizedMessages,
      summarizedMessages: preview.summarizedMessages,
      usedSummary: preview.usedSummary,
      usedFacts: false,
      factsCount: 0,
      keepRecent: config.keepRecent,
    }
  }

  const normalizedFacts = normalizeFactsState(facts, history.length)
  const prepared =
    config.strategy === 'facts'
      ? buildContextMessages(history, config, null, normalizedFacts)
      : history.slice(recentStart)
  const sentHistoryTokens = estimateMessagesTokens(prepared)
  const factsTokens =
    config.strategy === 'facts' && normalizedFacts?.facts.length
      ? estimateMessageTokens(factsMessage(normalizedFacts))
      : 0
  const tailTokens = Math.max(0, sentHistoryTokens - factsTokens)
  const savedTokens = fullHistoryTokens - sentHistoryTokens

  return {
    strategy: config.strategy,
    fullHistoryTokens,
    sentHistoryTokens,
    tailTokens,
    summaryTokens: 0,
    factsTokens,
    savedTokens,
    savedRatio: fullHistoryTokens > 0 ? savedTokens / fullHistoryTokens : 0,
    recentMessages: history.length - recentStart,
    droppedMessages: recentStart,
    summarizedMessages: 0,
    usedSummary: false,
    usedFacts: config.strategy === 'facts' && Boolean(normalizedFacts?.facts.length),
    factsCount: normalizedFacts?.facts.length ?? 0,
    keepRecent: config.keepRecent,
  }
}
