import type { ChatMessage } from '../types'

/**
 * Sticky Facts / Key-Value Memory.
 *
 * В отличие от сводки, facts — это не пересказ истории, а небольшая таблица
 * «ключ → значение»: цель проекта, сроки, бюджеты, ограничения, решения и
 * договорённости. Она обновляется после каждого сообщения пользователя, а в
 * запрос уходит вместе с последними N сообщениями.
 *
 * Facts хранятся отдельно от messages и переживают обрезку истории.
 */

export type FactMode = 'llm' | 'local'

export type FactCategory =
  | 'цель'
  | 'ограничение'
  | 'предпочтение'
  | 'решение'
  | 'договорённость'
  | 'имя'
  | 'срок'
  | 'бюджет'
  | 'риск'
  | 'другое'

export const FACT_CATEGORIES: FactCategory[] = [
  'цель',
  'ограничение',
  'предпочтение',
  'решение',
  'договорённость',
  'имя',
  'срок',
  'бюджет',
  'риск',
  'другое',
]

export interface StickyFact {
  /** Стабильный ключ: project.name, budget.total, decision.database… */
  key: string
  /** Значение в человекочитаемом виде. */
  value: string
  category: FactCategory
  /** Когда факт последний раз обновлялся. */
  updatedAt: string
}

export interface StickyFactsState {
  facts: StickyFact[]
  updatedAt: string
  /** Сколько сообщений истории уже обработано (включая assistant/tool). */
  processedMessages: number
  /** Сколько пользовательских сообщений дало хотя бы один факт. */
  updates: number
  /** Последний режим извлечения. */
  mode: FactMode
}

export interface FactExtractionOutput {
  facts: StickyFact[]
  mode: FactMode
  requests?: number
  promptTokens?: number
  completionTokens?: number
  costUsd?: number
  usageReported?: boolean
}

export interface FactExtractorInput {
  previousFacts: StickyFact[]
  message: ChatMessage
}

export type FactExtractor = (
  input: FactExtractorInput,
) => Promise<FactExtractionOutput>

export interface FactsUpdateStats {
  processedMessages: number
  newMessages: number
  userMessages: number
  updatedMessages: number
  addedFacts: number
  totalFacts: number
  requests: number
  promptTokens: number
  completionTokens: number
  costUsd: number
  usageReported: number
  mode: FactMode
  error?: string
}

export interface FactsUpdateOutcome {
  facts: StickyFactsState
  stats: FactsUpdateStats
  updated: boolean
}

export const EMPTY_FACTS: StickyFactsState = {
  facts: [],
  updatedAt: '',
  processedMessages: 0,
  updates: 0,
  mode: 'local',
}

const MAX_FACTS = 80

function clampInt(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min
  return Math.min(max, Math.max(min, Math.floor(value)))
}

export function isFactCategory(value: unknown): value is FactCategory {
  return typeof value === 'string' && (FACT_CATEGORIES as string[]).includes(value)
}

export function normalizeFactsState(
  state?: StickyFactsState | null,
  messageCount?: number,
): StickyFactsState | null {
  if (!state || !Array.isArray(state.facts)) return null

  const maxProcessed =
    typeof messageCount === 'number'
      ? Math.max(0, messageCount)
      : Number.MAX_SAFE_INTEGER

  const facts = state.facts
    .filter(
      (fact): fact is StickyFact =>
        Boolean(fact) &&
        typeof fact.key === 'string' &&
        fact.key.trim().length > 0 &&
        typeof fact.value === 'string' &&
        fact.value.trim().length > 0,
    )
    .map(fact => ({
      key: fact.key.trim(),
      value: fact.value.trim(),
      category: isFactCategory(fact.category) ? fact.category : 'другое',
      updatedAt:
        typeof fact.updatedAt === 'string' && fact.updatedAt
          ? fact.updatedAt
          : new Date().toISOString(),
    }))
    .slice(0, MAX_FACTS)

  return {
    facts,
    updatedAt: typeof state.updatedAt === 'string' ? state.updatedAt : '',
    processedMessages: clampInt(state.processedMessages ?? 0, 0, maxProcessed),
    updates: Math.max(0, clampInt(state.updates ?? 0, 0, 1_000_000)),
    mode: state.mode === 'llm' ? 'llm' : 'local',
  }
}

/** key → value в виде читаемого блока для системного сообщения. */
export function factsMessage(state: StickyFactsState): ChatMessage {
  const lines = state.facts.map(
    fact => `- ${fact.key} [${fact.category}]: ${fact.value}`,
  )
  return {
    role: 'system',
    content: `Долговременная память (sticky facts, ключ → значение). Считай эти факты известными и не переспрашивай их:\n${lines.join('\n')}`,
  }
}

export function factsToText(state: StickyFactsState | null): string {
  if (!state?.facts.length) return '(facts пока пусты)'
  return state.facts
    .map(fact => `- ${fact.key} [${fact.category}]: ${fact.value}`)
    .join('\n')
}

function normalizeKey(key: string, fallback: string): string {
  const cleaned = key
    .toLowerCase()
    .replace(/[^\p{L}\p{N}._-]+/gu, '.')
    .replace(/^\.+|\.+$/g, '')
    .slice(0, 64)
  return cleaned || fallback
}

function hashText(text: string): string {
  let hash = 0
  for (let index = 0; index < text.length; index += 1) {
    hash = (hash * 31 + text.charCodeAt(index)) | 0
  }
  return Math.abs(hash).toString(36).slice(0, 6)
}

/**
 * Мержит новые факты в старое состояние. Одинаковый ключ перезаписывается:
 * это позволяет обновлять бюджет или дедлайн, не плодя дубликаты.
 */
export function mergeFacts(
  previous: StickyFactsState | null,
  incoming: StickyFact[],
  nowIso: string,
  maxFacts = MAX_FACTS,
): { state: StickyFactsState; added: number; changed: number } {
  const base = previous ? normalizeFactsState(previous) ?? EMPTY_FACTS : EMPTY_FACTS
  const facts = base.facts.map(fact => ({ ...fact }))
  const indexByKey = new Map(facts.map((fact, index) => [fact.key, index]))
  let added = 0
  let changed = 0

  for (const raw of incoming) {
    const key = normalizeKey(raw.key, `fact.${hashText(raw.value)}`)
    const value = raw.value.trim()
    if (!key || !value) continue

    const category = isFactCategory(raw.category) ? raw.category : 'другое'
    const index = indexByKey.get(key)
    if (index === undefined) {
      indexByKey.set(key, facts.length)
      facts.push({
        key,
        value,
        category,
        updatedAt: raw.updatedAt || nowIso,
      })
      added += 1
      continue
    }

    const existing = facts[index]!
    if (existing.value !== value || existing.category !== category) {
      facts[index] = { key, value, category, updatedAt: nowIso }
      changed += 1
    }
  }

  const trimmed = facts.slice(Math.max(0, facts.length - maxFacts))
  return {
    state: {
      facts: trimmed,
      updatedAt: added > 0 || changed > 0 ? nowIso : base.updatedAt,
      processedMessages: base.processedMessages,
      updates: base.updates,
      mode: base.mode,
    },
    added,
    changed,
  }
}

/* ------------------------------------------------------------------ */
/* Локальное извлечение фактов                                          */
/* ------------------------------------------------------------------ */

function sentenceWith(text: string, pattern: RegExp): string {
  const sentences = text
    .split(/(?<=[.!?])\s+/)
    .map(sentence => sentence.trim())
    .filter(Boolean)
  const found = sentences.find(sentence => pattern.test(sentence))
  return found ?? text.trim()
}

function compact(text: string, limit = 220): string {
  const normalized = text.replace(/\s+/g, ' ').trim()
  if (normalized.length <= limit) return normalized
  return `${normalized.slice(0, Math.max(0, limit - 1)).trimEnd()}…`
}

function amount(value: string): string {
  return value.replace(/\s+/g, ' ').trim()
}

/**
 * Эвристическое извлечение без API. Оно намеренно консервативное: лучше
 * сохранить факт дословно, чем переформулировать и потерять число.
 *
 * Для демо-диалога этого достаточно, чтобы вытащить все 12 контрольных
 * деталей. В реальном приложении лучше LLM-режим: он понимает свободные
 * формулировки, а не только шаблоны ниже.
 */
export function extractFactsLocally(
  message: ChatMessage,
  nowIso = new Date().toISOString(),
): StickyFact[] {
  if (message.role !== 'user' || !message.content?.trim()) return []

  const text = message.content.trim()
  const facts: StickyFact[] = []
  const used = new Set<string>()

  const push = (key: string, value: string, category: FactCategory): void => {
    const normalizedValue = compact(value)
    if (!normalizedValue || used.has(key)) return
    used.add(key)
    facts.push({ key, value: normalizedValue, category, updatedAt: nowIso })
  }

  const project = text.match(/проект[а-яё]*\s*[«"]([^»"]+)[»"]/iu)
  if (project?.[1]) push('project.name', `«${project[1]}»`, 'имя')

  if (/запускаем\s+проект/iu.test(text)) {
    push('goal', sentenceWith(text, /запускаем\s+проект/iu), 'цель')
  }

  const customer = text.match(
    /заказчик\s*[—–-]\s*([А-ЯЁ][а-яёё-]+(?:\s+[А-ЯЁ][а-яёё-]+)?)/iu,
  )
  if (customer?.[1]) push('customer', customer[1], 'имя')

  const lead = text.match(
    /тимлид\s*[—–-]\s*([А-ЯЁ][а-яёё-]+(?:\s+[А-ЯЁ][а-яёё-]+)?)/iu,
  )
  if (lead?.[1]) push('team.lead', lead[1], 'имя')

  const deadline = text.match(
    /дедлайн[^.!?]*?([0-9]{1,2}\s+[а-яё]+\s+[0-9]{4})/iu,
  )
  if (deadline?.[1]) push('project.deadline', deadline[1], 'срок')

  const budget = text.match(/бюджет[^.!?]*?([0-9][0-9\s]{2,})\s*(?:руб|₽)/iu)
  if (budget?.[1]) {
    push('budget.total', `${amount(budget[1])} рублей`, 'бюджет')
  }

  const marketing = text.match(
    /маркетинг[^.!?]*?([0-9][0-9\s]{2,})\s*(?:руб|₽)/iu,
  )
  if (marketing?.[1]) {
    push('budget.marketing', `${amount(marketing[1])} рублей`, 'бюджет')
  }

  if (/российск[а-яё]*\s+ЦОД/iu.test(text) || /зарубежн[а-яё]*\s+облак/iu.test(text)) {
    push(
      'constraint.infra',
      sentenceWith(text, /российск[а-яё]*\s+ЦОД/iu),
      'ограничение',
    )
  }

  const team = text.match(
    /(?:в\s+команде\s*)?(\d+|один|одна|два|две|три|четыре|пять)\s+(?:разработчик|человек)/iu,
  )
  if (team?.[1]) {
    push('team.size', `${team[1]} разработчика`, 'ограничение')
  }

  const prototype = text.match(
    /прототип[^.!?]*?([0-9]{1,2}\s+[а-яё]+\s+[0-9]{4})/iu,
  )
  if (prototype?.[1]) {
    push('milestone.prototype', prototype[1], 'срок')
  }

  const release = text.match(
    /релиз[^.!?]*?([0-9]{1,2}\s+[а-яё]+\s+[0-9]{4})/iu,
  )
  if (release?.[1]) {
    push('milestone.release', release[1], 'срок')
  }

  const database = text.match(
    /\b(PostgreSQL|MySQL|MongoDB|SQLite|Oracle|ClickHouse)\b/iu,
  )
  if (database?.[1] && /(?:выбрали|выбор|баз[ае]?\s+данн|не\s+бер)/iu.test(text)) {
    push('decision.database', sentenceWith(text, /PostgreSQL|MySQL|MongoDB|SQLite|Oracle|ClickHouse/iu), 'решение')
  }

  if (/\bREST\b/iu.test(text) || /\bGraphQL\b/iu.test(text)) {
    push('decision.api', sentenceWith(text, /\bREST\b|\bGraphQL\b/iu), 'решение')
  }

  if (/риск|блокер|критичн/iu.test(text)) {
    push('risk.status', sentenceWith(text, /риск|блокер|критичн/iu), 'риск')
  }

  if (/созвон|встреч[а-яё]*/iu.test(text)) {
    push('agreement.meeting', sentenceWith(text, /созвон|встреч[а-яё]*/iu), 'договорённость')
  }

  if (/предпочита|удобнее|хотелось\s+бы|лучше/iu.test(text)) {
    push(
      `preference.${hashText(sentenceWith(text, /предпочита|удобнее|хотелось\s+бы|лучше/iu))}`,
      sentenceWith(text, /предпочита|удобнее|хотелось\s+бы|лучше/iu),
      'предпочтение',
    )
  }

  // Общие ограничения и решения — на случай свободных формулировок, которых
  // нет в шаблонах выше. Ключ стабилен по тексту предложения.
  if (/ограничени|запрещ|нельзя|только/iu.test(text)) {
    const sentence = sentenceWith(text, /ограничени|запрещ|нельзя|только/iu)
    push(`constraint.${hashText(sentence)}`, sentence, 'ограничение')
  }

  if (/договорились|согласовали|зафиксировали/iu.test(text)) {
    const sentence = sentenceWith(text, /договорились|согласовали|зафиксировали/iu)
    push(`agreement.${hashText(sentence)}`, sentence, 'договорённость')
  }

  return facts
}

/* ------------------------------------------------------------------ */
/* Обновление памяти после сообщений пользователя                       */
/* ------------------------------------------------------------------ */

export interface UpdateFactsInput {
  messages: ChatMessage[]
  previous?: StickyFactsState | null
  mode?: FactMode
  extractor?: FactExtractor
  maxFacts?: number
  now?: () => Date
}

export interface UpdateFactsCallbacks {
  onExtractionStart?: (message: ChatMessage) => void
  onExtractionEnd?: (message: ChatMessage, error?: Error) => void
}

/**
 * Проходит по ещё не обработанным сообщениям и обновляет facts после каждого
 * пользовательского сообщения. В LLM-режиме это один запрос на сообщение —
 * именно поэтому «сложность» стратегии выше, чем у sliding window.
 */
export async function updateFacts(
  input: UpdateFactsInput,
  callbacks: UpdateFactsCallbacks = {},
): Promise<FactsUpdateOutcome> {
  const now = input.now ?? (() => new Date())
  const requestedMode = input.mode ?? (input.extractor ? 'llm' : 'local')
  const mode: FactMode = input.extractor ? requestedMode : 'local'
  const maxFacts = Math.max(1, Math.floor(input.maxFacts ?? MAX_FACTS))
  const messages = input.messages.filter(message => message.role !== 'system')
  let previous = normalizeFactsState(input.previous, messages.length)

  // История могла укоротиться (импорт/смена ветки): не считаем её уже
  // обработанной, но старые facts сохраняем — они всё ещё полезны.
  if (previous && previous.processedMessages > messages.length) {
    previous = { ...previous, processedMessages: 0 }
  }

  const start = previous?.processedMessages ?? 0
  let state =
    previous ??
    ({
      ...EMPTY_FACTS,
      mode,
    } satisfies StickyFactsState)

  let requests = 0
  let promptTokens = 0
  let completionTokens = 0
  let costUsd = 0
  let usageReported = 0
  let updatedMessages = 0
  let addedFacts = 0
  let error: string | undefined

  for (let index = start; index < messages.length; index += 1) {
    const message = messages[index]!
    state = { ...state, processedMessages: index + 1 }
    if (message.role !== 'user') continue

    let incoming: StickyFact[] = []
    if (input.extractor) {
      callbacks.onExtractionStart?.(message)
      try {
        const output = await input.extractor({
          previousFacts: state.facts,
          message,
        })
        incoming = output.facts
        requests += output.requests ?? 1
        promptTokens += output.promptTokens ?? 0
        completionTokens += output.completionTokens ?? 0
        costUsd += output.costUsd ?? 0
        if (output.usageReported) usageReported += 1
        state = { ...state, mode: output.mode }
        callbacks.onExtractionEnd?.(message)
      } catch (caught) {
        error = caught instanceof Error ? caught.message : String(caught)
        callbacks.onExtractionEnd?.(message, caught as Error)
        // Как и у summary: сбой LLM не должен ломать диалог.
        incoming = extractFactsLocally(message, now().toISOString())
        state = { ...state, mode: 'local' }
      }
    } else {
      incoming = extractFactsLocally(message, now().toISOString())
      state = { ...state, mode }
    }

    if (!incoming.length) continue

    const merged = mergeFacts(state, incoming, now().toISOString(), maxFacts)
    if (merged.added + merged.changed > 0) {
      updatedMessages += 1
      addedFacts += merged.added
    }
    state = {
      ...merged.state,
      processedMessages: state.processedMessages,
      updates: state.updates + (merged.added + merged.changed > 0 ? 1 : 0),
      mode: state.mode,
    }
  }

  state = {
    ...state,
    processedMessages: messages.length,
    mode: input.extractor ? state.mode : 'local',
  }

  return {
    facts: state,
    updated: updatedMessages > 0,
    stats: {
      processedMessages: messages.length,
      newMessages: messages.length - start,
      userMessages: messages.slice(start).filter(message => message.role === 'user').length,
      updatedMessages,
      addedFacts,
      totalFacts: state.facts.length,
      requests,
      promptTokens,
      completionTokens,
      costUsd,
      usageReported,
      mode: state.mode,
      ...(error ? { error } : {}),
    },
  }
}
