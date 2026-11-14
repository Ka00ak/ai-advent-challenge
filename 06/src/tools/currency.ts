import type { AgentTool } from '../types'

/**
 * Зашитые курсы «сколько единиц валюты за 1 USD».
 * Это учебный инструмент: курсы не обновляются из интернета.
 */
const RATES_PER_USD: Record<string, number> = {
  USD: 1,
  EUR: 0.92,
  GBP: 0.79,
  RUB: 92.5,
  CNY: 7.1,
  JPY: 155,
  KZT: 480,
  TRY: 34,
  INR: 84,
  BYN: 3.27,
}

const SUPPORTED = Object.keys(RATES_PER_USD).join(', ')

function round(value: number, digits: number): number {
  const factor = 10 ** digits
  return Math.round(value * factor) / factor
}

export const convertCurrencyTool: AgentTool = {
  name: 'convert_currency',
  description:
    'Конвертирует сумму между валютами по зашитому в код курсу. ' +
    `Поддерживаются: ${SUPPORTED}. Курс фиксированный и не обновляется в реальном времени.`,
  parameters: {
    type: 'object',
    properties: {
      amount: {
        type: 'number',
        description: 'Сумма для конвертации',
      },
      from: {
        type: 'string',
        description: `Код исходной валюты: ${SUPPORTED}`,
      },
      to: {
        type: 'string',
        description: `Код целевой валюты: ${SUPPORTED}`,
      },
    },
    required: ['amount', 'from', 'to'],
    additionalProperties: false,
  },
  execute(args) {
    const amount = Number(args.amount)
    if (!Number.isFinite(amount)) throw new Error('Параметр amount должен быть числом')

    const from = String(args.from ?? '').trim().toUpperCase()
    const to = String(args.to ?? '').trim().toUpperCase()
    const fromRate = RATES_PER_USD[from]
    const toRate = RATES_PER_USD[to]

    if (!fromRate) throw new Error(`Валюта «${from}» не поддерживается. Доступны: ${SUPPORTED}`)
    if (!toRate) throw new Error(`Валюта «${to}» не поддерживается. Доступны: ${SUPPORTED}`)

    const rate = toRate / fromRate
    return {
      amount,
      from,
      to,
      rate: round(rate, 6),
      result: round(amount * rate, 4),
      note: 'Курс зашит в код и не является актуальным.',
    }
  },
}
