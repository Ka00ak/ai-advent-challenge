import { ToolRegistry } from '../agent/ToolRegistry'
import { calculatorTool } from './calculator'
import { convertCurrencyTool } from './currency'
import { currentTimeTool } from './time'

export const DEFAULT_TOOLS = [currentTimeTool, calculatorTool, convertCurrencyTool]

/** Новый реестр на каждый запуск агента — без общего изменяемого состояния. */
export function createDefaultToolRegistry(): ToolRegistry {
  return new ToolRegistry(DEFAULT_TOOLS)
}

/** Короткие описания для панели настроек. */
export function listDefaultTools(): Array<{ name: string; description: string }> {
  return DEFAULT_TOOLS.map(tool => ({ name: tool.name, description: tool.description }))
}
