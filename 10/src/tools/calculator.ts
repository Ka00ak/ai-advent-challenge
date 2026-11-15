import type { AgentTool } from '../types'

interface MathFunction {
  minArgs: number
  maxArgs: number
  apply: (...args: number[]) => number
}

const FUNCTIONS: Record<string, MathFunction> = {
  sqrt: { minArgs: 1, maxArgs: 1, apply: Math.sqrt },
  abs: { minArgs: 1, maxArgs: 1, apply: Math.abs },
  floor: { minArgs: 1, maxArgs: 1, apply: Math.floor },
  ceil: { minArgs: 1, maxArgs: 1, apply: Math.ceil },
  min: { minArgs: 1, maxArgs: Number.POSITIVE_INFINITY, apply: Math.min },
  max: { minArgs: 1, maxArgs: Number.POSITIVE_INFINITY, apply: Math.max },
  round: {
    minArgs: 1,
    maxArgs: 2,
    apply: (value: number, digits = 0) => {
      const factor = 10 ** Math.trunc(digits)
      return Math.round(value * factor) / factor
    },
  },
}

const CONSTANTS: Record<string, number> = {
  pi: Math.PI,
  e: Math.E,
}

/**
 * Рекурсивный парсер арифметики. Никакого eval/new Function:
 * поддерживаются числа, + - * / % ^, скобки, унарный минус,
 * константы pi/e и небольшой набор функций.
 */
class ExpressionParser {
  private readonly source: string
  private position = 0

  constructor(source: string) {
    this.source = source
  }

  parse(): number {
    const value = this.parseExpression()
    this.skipSpaces()
    if (this.position < this.source.length) {
      throw new Error(
        `Неожиданный символ «${this.source[this.position]}» в позиции ${this.position + 1}`,
      )
    }
    return value
  }

  private parseExpression(): number {
    let value = this.parseTerm()
    for (;;) {
      this.skipSpaces()
      const operator = this.peek()
      if (operator !== '+' && operator !== '-') return value
      this.position += 1
      const right = this.parseTerm()
      value = operator === '+' ? value + right : value - right
    }
  }

  private parseTerm(): number {
    let value = this.parseUnary()
    for (;;) {
      this.skipSpaces()
      const operator = this.peek()
      if (operator !== '*' && operator !== '/' && operator !== '%') return value
      this.position += 1
      const right = this.parseUnary()
      if ((operator === '/' || operator === '%') && right === 0) {
        throw new Error('Деление на ноль')
      }
      if (operator === '*') value *= right
      else if (operator === '/') value /= right
      else value %= right
    }
  }

  private parseUnary(): number {
    this.skipSpaces()
    const symbol = this.peek()
    if (symbol === '+' || symbol === '-') {
      this.position += 1
      const value = this.parseUnary()
      return symbol === '-' ? -value : value
    }
    return this.parsePower()
  }

  private parsePower(): number {
    const base = this.parsePrimary()
    this.skipSpaces()
    if (this.peek() !== '^') return base
    this.position += 1
    const exponent = this.parseUnary()
    return base ** exponent
  }

  private parsePrimary(): number {
    this.skipSpaces()
    const symbol = this.peek()
    if (symbol === '(') {
      this.position += 1
      const value = this.parseExpression()
      this.skipSpaces()
      if (this.peek() !== ')') throw new Error('Не закрыта скобка')
      this.position += 1
      return value
    }
    if (symbol !== undefined && /[0-9.]/.test(symbol)) return this.parseNumber()
    if (symbol !== undefined && /[a-zA-Z]/.test(symbol)) return this.parseIdentifier()
    if (symbol === undefined) throw new Error('Неожиданный конец выражения')
    throw new Error(`Ожидалось число, скобка или функция, получено «${symbol}»`)
  }

  private parseNumber(): number {
    const start = this.position
    let dots = 0
    while (this.position < this.source.length) {
      const symbol = this.source[this.position] as string
      if (symbol === '.') {
        dots += 1
        this.position += 1
      } else if (/[0-9]/.test(symbol)) {
        this.position += 1
      } else {
        break
      }
    }
    const raw = this.source.slice(start, this.position)
    if (dots > 1 || raw === '.' || raw === '') {
      throw new Error(`Некорректное число «${raw}»`)
    }
    return Number(raw)
  }

  private parseIdentifier(): number {
    const start = this.position
    while (
      this.position < this.source.length &&
      /[a-zA-Z0-9_]/.test(this.source[this.position] as string)
    ) {
      this.position += 1
    }
    const name = this.source.slice(start, this.position).toLowerCase()
    this.skipSpaces()

    if (this.peek() !== '(') {
      const constant = CONSTANTS[name]
      if (constant === undefined) throw new Error(`Неизвестная константа «${name}»`)
      return constant
    }

    this.position += 1
    const args: number[] = []
    this.skipSpaces()
    if (this.peek() === ')') {
      this.position += 1
    } else {
      for (;;) {
        args.push(this.parseExpression())
        this.skipSpaces()
        const symbol = this.peek()
        if (symbol === ',') {
          this.position += 1
          continue
        }
        if (symbol === ')') {
          this.position += 1
          break
        }
        throw new Error('Ожидалась запятая или закрывающая скобка')
      }
    }

    const fn = FUNCTIONS[name]
    if (!fn) throw new Error(`Неизвестная функция «${name}»`)
    if (args.length < fn.minArgs || args.length > fn.maxArgs) {
      throw new Error(
        `Функция «${name}» ожидает от ${fn.minArgs} до ${fn.maxArgs} аргументов, получено ${args.length}`,
      )
    }
    return fn.apply(...args)
  }

  private skipSpaces(): void {
    while (
      this.position < this.source.length &&
      /\s/.test(this.source[this.position] as string)
    ) {
      this.position += 1
    }
  }

  private peek(): string | undefined {
    return this.source[this.position]
  }
}

/** Публичная функция — удобно тестировать отдельно от агента. */
export function evaluateExpression(expression: string): number {
  if (!expression.trim()) throw new Error('Пустое выражение')
  const value = new ExpressionParser(expression).parse()
  if (!Number.isFinite(value)) throw new Error('Результат не является конечным числом')
  // Убираем хвосты вида 0.30000000000000004.
  return Number(value.toPrecision(12))
}

export const calculatorTool: AgentTool = {
  name: 'calculate',
  description:
    'Вычисляет арифметическое выражение. Поддерживает + - * / % ^, скобки, ' +
    'функции sqrt, abs, round, floor, ceil, min, max и константы pi, e. ' +
    'Примеры: "2 + 2 * 2", "sqrt(16)", "round(1234.567, 2)".',
  parameters: {
    type: 'object',
    properties: {
      expression: {
        type: 'string',
        description: 'Арифметическое выражение, например (250 * 1.15) / 0.92',
      },
    },
    required: ['expression'],
    additionalProperties: false,
  },
  execute(args) {
    const expression = String(args.expression ?? '').trim()
    if (!expression) throw new Error('Параметр expression не может быть пустым')
    return { expression, result: evaluateExpression(expression) }
  },
}
