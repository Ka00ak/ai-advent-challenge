import type { ChatMessage } from '../types'

const INTERRUPTED_TOOL_RESULT = JSON.stringify({
  ok: false,
  error: 'Вызов инструмента не был завершён: диалог прерван или приложение перезапущено.',
})

/**
 * Приводит историю к валидному для Chat Completions виду.
 *
 * Если ассистент попросил вызвать инструменты, но из-за отмены, ошибки или
 * перезапуска приложения tool-сообщений не хватает, добавляем недостающие
 * ответы с ошибкой. Иначе следующий запрос к API может быть отклонён:
 * на каждый tool_call должен быть ровно один tool-результат.
 */
export function repairToolCallSequence(messages: ChatMessage[]): ChatMessage[] {
  const repaired: ChatMessage[] = []
  let index = 0

  while (index < messages.length) {
    const message = messages[index]
    repaired.push(message)

    if (message.role !== 'assistant' || !message.tool_calls?.length) {
      index += 1
      continue
    }

    const answered = new Set<string>()
    let next = index + 1
    while (next < messages.length && messages[next]?.role === 'tool') {
      const toolCallId = messages[next]?.tool_call_id
      if (toolCallId) answered.add(toolCallId)
      repaired.push(messages[next])
      next += 1
    }

    for (const call of message.tool_calls) {
      if (answered.has(call.id)) continue
      repaired.push({
        role: 'tool',
        tool_call_id: call.id,
        name: call.function.name,
        content: INTERRUPTED_TOOL_RESULT,
      })
    }

    index = next
  }

  return repaired
}
