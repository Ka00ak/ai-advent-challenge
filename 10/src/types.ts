/** Общие типы для агента, инструментов и журнала API-запросов. */

import type { CompressionRunStats, ContextSummary } from './agent/compression'
import type { ContextBreakdown } from './tokens'

export type ChatRole = 'system' | 'user' | 'assistant' | 'tool'

export interface ToolCall {
  id: string
  type: 'function'
  function: {
    name: string
    arguments: string
  }
}

export interface ChatMessage {
  role: ChatRole
  content: string | null
  tool_calls?: ToolCall[]
  tool_call_id?: string
  name?: string
}

export interface JsonSchema {
  type: string
  description?: string
  properties?: Record<string, JsonSchema>
  required?: string[]
  enum?: string[]
  items?: JsonSchema
  default?: unknown
  minimum?: number
  maximum?: number
  additionalProperties?: boolean
}

export interface ToolSchema {
  type: 'function'
  function: {
    name: string
    description: string
    parameters: JsonSchema
  }
}

export interface ToolSummary {
  name: string
  description: string
}

/** Инструмент агента: схема для модели + обычная функция-исполнитель. */
export interface AgentTool<Args = Record<string, unknown>, Result = unknown> {
  name: string
  description: string
  parameters: JsonSchema
  execute(args: Args): Result | Promise<Result>
}

export interface TokenUsage {
  prompt_tokens?: number
  completion_tokens?: number
  total_tokens?: number
}

/** Один запрос к LLM и полученный на него ответ (для журнала и отладки). */
export interface LlmExchange {
  id: string
  /** chat — обычный запрос агента, summary — запрос на сжатие истории. */
  kind?: 'chat' | 'summary'
  url: string
  startedAt: string
  durationMs: number
  request: LlmRequest
  response?: unknown
  message?: ChatMessage
  usage?: TokenUsage
  /** Оценка промпта до отправки: видно, из чего сложились токены. */
  estimate?: ContextBreakdown
  /** Оценка стоимости этого запроса в USD. */
  costUsd?: number
  error?: string
}

export interface LlmRequest {
  model: string
  messages: ChatMessage[]
  tools?: ToolSchema[]
  tool_choice?: 'auto' | 'none'
  temperature?: number
  max_tokens?: number
}

/** Результат выполнения одного вызова инструмента. */
export interface ToolInvocation {
  id: string
  name: string
  arguments: string
  startedAt: string
  durationMs: number
  ok: boolean
  result?: unknown
  error?: string
}

export interface AgentStep {
  iteration: number
  exchange: LlmExchange
  toolResults: ToolInvocation[]
  /** Шаг восстановлен из сохранённой истории, а не получен в текущем запуске. */
  restored?: boolean
}

export type AgentStopReason = 'final' | 'max-iterations' | 'cancelled'

/** Токены одного запроса агента (одна итерация tool-loop). */
export interface ExchangeTokenStat {
  iteration: number
  /** Оценка до отправки — работает даже без usage в ответе. */
  estimatedPromptTokens: number
  promptTokens: number
  completionTokens: number
  totalTokens: number
  costUsd: number
  /** true, если провайдер вернул точный usage. */
  usageReported: boolean
}

/** Итог по токенам за один запуск агента (может быть несколько итераций). */
export interface AgentTokenStats {
  requests: ExchangeTokenStat[]
  promptTokens: number
  completionTokens: number
  totalTokens: number
  estimatedPromptTokens: number
  costUsd: number
  reportedRequests: number
  estimatedRequests: number
}

export interface AgentRunResult {
  finalText: string
  steps: AgentStep[]
  /** Полная история, включая сжатые сообщения: её и нужно сохранять. */
  messages: ChatMessage[]
  stoppedReason: AgentStopReason
  tokenStats: AgentTokenStats
  /** Актуальная сводка: хранится отдельно от messages. */
  summary: ContextSummary | null
  /** Что именно ушло в запрос и сколько на этом сэкономлено. */
  compression: CompressionRunStats
}

export interface LlmSettings {
  baseUrl: string
  apiKey: string
  model: string
  temperature: number
  maxTokens: number | null
  maxIterations: number
}
