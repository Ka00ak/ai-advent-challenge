import type {
  AgentTool,
  ToolCall,
  ToolInvocation,
  ToolSchema,
  ToolSummary,
} from '../types'

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function elapsedMs(started: number): number {
  return Math.round(performance.now() - started)
}

/**
 * Реестр инструментов: хранит схемы для модели и умеет выполнять вызовы.
 * Ошибки инструментов не выбрасываются наружу — они возвращаются модели,
 * чтобы она могла исправиться или объяснить проблему пользователю.
 */
export class ToolRegistry {
  private readonly tools = new Map<string, AgentTool>()

  constructor(tools: AgentTool[] = []) {
    for (const tool of tools) this.register(tool)
  }

  register(tool: AgentTool): void {
    if (this.tools.has(tool.name)) {
      throw new Error(`Инструмент «${tool.name}» уже зарегистрирован`)
    }
    this.tools.set(tool.name, tool)
  }

  list(): AgentTool[] {
    return [...this.tools.values()]
  }

  summaries(): ToolSummary[] {
    return this.list().map(tool => ({ name: tool.name, description: tool.description }))
  }

  schemas(): ToolSchema[] {
    return this.list().map(tool => ({
      type: 'function',
      function: {
        name: tool.name,
        description: tool.description,
        parameters: tool.parameters,
      },
    }))
  }

  async invoke(call: ToolCall): Promise<ToolInvocation> {
    const startedAt = new Date().toISOString()
    const started = performance.now()
    const name = call.function?.name ?? ''
    const rawArguments = call.function?.arguments ?? '{}'
    const base = {
      id: call.id,
      name,
      arguments: rawArguments,
      startedAt,
    }

    const tool = this.tools.get(name)
    if (!tool) {
      return {
        ...base,
        durationMs: elapsedMs(started),
        ok: false,
        error: `Неизвестный инструмент: ${name || '(пустое имя)'}`,
      }
    }

    let args: Record<string, unknown>
    try {
      const parsed: unknown = rawArguments.trim() ? JSON.parse(rawArguments) : {}
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        throw new Error('аргументы должны быть JSON-объектом')
      }
      args = parsed as Record<string, unknown>
    } catch (error) {
      return {
        ...base,
        durationMs: elapsedMs(started),
        ok: false,
        error: `Некорректные аргументы: ${errorMessage(error)}`,
      }
    }

    try {
      const result = await tool.execute(args)
      return {
        ...base,
        durationMs: elapsedMs(started),
        ok: true,
        result,
      }
    } catch (error) {
      return {
        ...base,
        durationMs: elapsedMs(started),
        ok: false,
        error: errorMessage(error),
      }
    }
  }
}
