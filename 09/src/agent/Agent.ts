import { buildEndpoint, createChatCompletion } from '../api/llm'
import {
  buildPromptEstimate,
  estimateCost,
  estimateTextTokens,
  resolveModelProfile,
  type ContextBreakdown,
  type ModelProfile,
} from '../tokens'
import type {
  AgentRunResult,
  AgentStep,
  AgentStopReason,
  ChatMessage,
  ExchangeTokenStat,
  LlmExchange,
  LlmRequest,
  LlmSettings,
  ToolCall,
  ToolInvocation,
} from '../types'
import { repairToolCallSequence } from './history'
import { ToolRegistry } from './ToolRegistry'

export interface AgentConfig extends LlmSettings {
  systemPrompt: string
  /** Ручное переопределение контекстного окна. null — взять из профиля модели. */
  contextLimit?: number | null
}

export interface AgentCallbacks {
  onStatus?: (text: string) => void
  /** Оценка промпта перед каждым запросом — для индикатора заполнения контекста. */
  onPromptEstimate?: (estimate: ContextBreakdown, iteration: number) => void
  onExchangeStart?: (exchange: LlmExchange) => void
  onExchangeEnd?: (exchange: LlmExchange) => void
  onToolStart?: (invocation: ToolInvocation) => void
  onToolEnd?: (invocation: ToolInvocation) => void
  onFinalText?: (text: string) => void
  /** Каждое новое сообщение (user/assistant/tool) — для сохранения контекста на лету. */
  onMessageAppended?: (message: ChatMessage) => void
}

function makeId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `id-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function toErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error)
}

function isAbortError(error: unknown): boolean {
  return (
    (error instanceof DOMException && error.name === 'AbortError') ||
    (error instanceof Error && error.name === 'AbortError')
  )
}

/** API может вернуть content строкой или массивом частей — приводим к тексту. */
function contentToText(content: unknown): string | null {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    const parts = content.map(part => {
      if (typeof part === 'string') return part
      if (part && typeof part === 'object' && 'text' in part) {
        const text = (part as { text?: unknown }).text
        if (typeof text === 'string') return text
      }
      return ''
    })
    return parts.join('')
  }
  return null
}

function normalizeToolCalls(calls: ToolCall[] | undefined): ToolCall[] {
  if (!Array.isArray(calls)) return []
  return calls.map((call, index) => {
    const rawArguments: unknown = call.function?.arguments
    return {
      id: call.id || `call_${Date.now()}_${index}`,
      type: 'function',
      function: {
        name: call.function?.name ?? '',
        arguments:
          typeof rawArguments === 'string' ? rawArguments : JSON.stringify(rawArguments ?? {}),
      },
    }
  })
}

/**
 * Агент — отдельная сущность, инкапсулирующая цикл:
 * запрос к модели → вызов инструментов → возврат результатов → новый запрос,
 * пока модель не перестанет просить инструменты.
 */
export class Agent {
  private readonly config: AgentConfig
  private readonly registry: ToolRegistry

  constructor(config: AgentConfig, registry: ToolRegistry) {
    this.config = config
    this.registry = registry
  }

  async run(
    userText: string,
    history: ChatMessage[],
    callbacks: AgentCallbacks = {},
    signal?: AbortSignal,
  ): Promise<AgentRunResult> {
    const steps: AgentStep[] = []
    const messages: ChatMessage[] = [
      { role: 'system', content: this.config.systemPrompt },
      ...repairToolCallSequence(history.filter(message => message.role !== 'system')),
    ]
    const append = (message: ChatMessage): void => {
      messages.push(message)
      callbacks.onMessageAppended?.(message)
    }

    append({ role: 'user', content: userText })

    const tools = this.registry.schemas()
    const maxIterations = Math.max(1, Math.floor(this.config.maxIterations))
    const profile: ModelProfile = resolveModelProfile(
      this.config.model,
      this.config.contextLimit,
    )

    let finalText = ''
    let stoppedReason: AgentStopReason = 'max-iterations'
    const tokenRequests: ExchangeTokenStat[] = []
    let promptTokensTotal = 0
    let completionTokensTotal = 0
    let estimatedPromptTotal = 0
    let costTotal = 0
    let reportedRequests = 0
    let estimatedRequests = 0

    for (let iteration = 1; iteration <= maxIterations; iteration += 1) {
      if (signal?.aborted) {
        stoppedReason = 'cancelled'
        break
      }

      // Оценка до отправки нужна для индикатора заполнения контекста.
      // Даже при переполнении отправляем запрос: пусть решение принимает
      // провайдер — он либо ответит, либо вернёт context_length_exceeded.
      const estimate = buildPromptEstimate(messages, tools, profile.contextLimit)
      callbacks.onPromptEstimate?.(estimate, iteration)

      callbacks.onStatus?.(`Запрос к модели (итерация ${iteration} из ${maxIterations})…`)

      const request: LlmRequest = {
        model: this.config.model,
        messages: structuredClone(messages),
        temperature: this.config.temperature,
        ...(this.config.maxTokens !== null && this.config.maxTokens > 0
          ? { max_tokens: Math.floor(this.config.maxTokens) }
          : {}),
        ...(tools.length > 0 ? { tools, tool_choice: 'auto' as const } : {}),
      }

      const exchange: LlmExchange = {
        id: makeId(),
        url: buildEndpoint(this.config.baseUrl),
        startedAt: new Date().toISOString(),
        durationMs: 0,
        request,
        estimate,
      }
      callbacks.onExchangeStart?.(exchange)
      const started = performance.now()

      let response
      try {
        response = await createChatCompletion(
          this.config.baseUrl,
          this.config.apiKey,
          request,
          signal,
        )
      } catch (error) {
        exchange.durationMs = Math.round(performance.now() - started)
        exchange.error = isAbortError(error) ? 'Запрос отменён' : toErrorMessage(error)
        callbacks.onExchangeEnd?.(exchange)
        throw error
      }

      exchange.durationMs = Math.round(performance.now() - started)
      exchange.response = response
      exchange.usage = response.usage

      const choice = response.choices?.[0]
      if (!choice?.message) {
        exchange.error = 'API не вернул message в choices[0]'
        callbacks.onExchangeEnd?.(exchange)
        throw new Error(exchange.error)
      }

      const toolCalls = normalizeToolCalls(choice.message.tool_calls)
      const assistantMessage: ChatMessage = {
        role: 'assistant',
        content: contentToText(choice.message.content),
        ...(toolCalls.length > 0 ? { tool_calls: toolCalls } : {}),
      }
      exchange.message = assistantMessage

      // usage от провайдера — точные токены; если его нет, показываем оценку.
      const usage = response.usage
      const usageReported = typeof usage?.prompt_tokens === 'number'
      const promptTokens = usage?.prompt_tokens ?? estimate.totalTokens
      const estimatedCompletion =
        estimateTextTokens(assistantMessage.content ?? '') +
        toolCalls.reduce(
          (sum, call) =>
            sum +
            estimateTextTokens(call.function.name) +
            estimateTextTokens(call.function.arguments),
          0,
        )
      const completionTokens = usage?.completion_tokens ?? estimatedCompletion
      const totalTokens = usage?.total_tokens ?? promptTokens + completionTokens
      const costUsd = estimateCost(profile, promptTokens, completionTokens).totalUsd

      exchange.costUsd = costUsd
      tokenRequests.push({
        iteration,
        estimatedPromptTokens: estimate.totalTokens,
        promptTokens,
        completionTokens,
        totalTokens,
        costUsd,
        usageReported,
      })
      promptTokensTotal += promptTokens
      completionTokensTotal += completionTokens
      estimatedPromptTotal += estimate.totalTokens
      costTotal += costUsd
      if (usageReported) reportedRequests += 1
      else estimatedRequests += 1

      callbacks.onExchangeEnd?.(exchange)
      append(assistantMessage)

      const step: AgentStep = { iteration, exchange, toolResults: [] }
      steps.push(step)

      if (toolCalls.length === 0) {
        finalText = assistantMessage.content?.trim() ?? ''
        stoppedReason = 'final'
        callbacks.onFinalText?.(finalText)
        break
      }

      for (const call of toolCalls) {
        if (signal?.aborted) {
          stoppedReason = 'cancelled'
          break
        }

        callbacks.onStatus?.(
          `Вызов инструмента «${call.function.name}» (итерация ${iteration})…`,
        )

        const pending: ToolInvocation = {
          id: call.id,
          name: call.function.name,
          arguments: call.function.arguments,
          startedAt: new Date().toISOString(),
          durationMs: 0,
          ok: false,
        }
        callbacks.onToolStart?.(pending)

        const invocation = await this.registry.invoke(call)
        Object.assign(pending, invocation)
        step.toolResults.push(pending)
        callbacks.onToolEnd?.(pending)

        append({
          role: 'tool',
          tool_call_id: call.id,
          name: call.function.name,
          content: JSON.stringify(
            invocation.ok
              ? { ok: true, result: invocation.result }
              : { ok: false, error: invocation.error },
          ),
        })
      }

      if (signal?.aborted) {
        stoppedReason = 'cancelled'
        break
      }
    }

    return {
      finalText,
      steps,
      messages: messages.filter(message => message.role !== 'system'),
      stoppedReason,
      tokenStats: {
        requests: tokenRequests,
        promptTokens: promptTokensTotal,
        completionTokens: completionTokensTotal,
        totalTokens: tokenRequests.reduce((sum, request) => sum + request.totalTokens, 0),
        estimatedPromptTokens: estimatedPromptTotal,
        costUsd: costTotal,
        reportedRequests,
        estimatedRequests,
      },
    }
  }
}
