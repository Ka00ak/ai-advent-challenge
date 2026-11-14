import type { AgentTool } from '../types'

function isValidTimeZone(timeZone: string): boolean {
  try {
    new Intl.DateTimeFormat('ru-RU', { timeZone }).format(new Date())
    return true
  } catch {
    return false
  }
}

export const currentTimeTool: AgentTool = {
  name: 'get_current_time',
  description:
    'Возвращает текущие дату и время, unix timestamp и день недели. ' +
    'Можно указать IANA-таймзону: Europe/Moscow, Asia/Tokyo, UTC и т.п. ' +
    'Без параметра используется локальная таймзона пользователя.',
  parameters: {
    type: 'object',
    properties: {
      timezone: {
        type: 'string',
        description: 'IANA-таймзона, например Europe/Moscow. Необязательно.',
      },
    },
    additionalProperties: false,
  },
  execute(args) {
    const requested =
      typeof args.timezone === 'string' && args.timezone.trim()
        ? args.timezone.trim()
        : null

    if (requested && !isValidTimeZone(requested)) {
      throw new Error(`Неизвестная таймзона «${requested}». Используй IANA, например Europe/Moscow`)
    }

    const now = new Date()
    const timeZone = requested ?? Intl.DateTimeFormat().resolvedOptions().timeZone
    const formatted = new Intl.DateTimeFormat('ru-RU', {
      timeZone,
      dateStyle: 'full',
      timeStyle: 'long',
    }).format(now)

    const weekday = new Intl.DateTimeFormat('ru-RU', {
      timeZone,
      weekday: 'long',
    }).format(now)

    return {
      timezone: timeZone,
      formatted,
      weekday,
      iso: now.toISOString(),
      unix: Math.floor(now.getTime() / 1000),
    }
  },
}
