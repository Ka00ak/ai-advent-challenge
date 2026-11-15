import { describe, expect, it } from 'vitest'
import { evaluateExpression } from '../src/tools/calculator'

describe('evaluateExpression', () => {
  it('учитывает приоритет операторов', () => {
    expect(evaluateExpression('2 + 2 * 2')).toBe(6)
  })

  it('поддерживает скобки и унарный минус', () => {
    expect(evaluateExpression('-(2 + 3) * 4')).toBe(-20)
    expect(evaluateExpression('2 ^ 3 ^ 2')).toBe(512)
  })

  it('поддерживает функции, константы и округление', () => {
    expect(evaluateExpression('round(sqrt(16) + 1.2345, 2)')).toBe(5.23)
    expect(evaluateExpression('round(pi, 4)')).toBe(3.1416)
    expect(evaluateExpression('max(2, 7, 4) + min(1, 3)')).toBe(8)
  })

  it('считает сценарий «валюта → проценты»', () => {
    expect(evaluateExpression('(250 * 0.92) * 1.15')).toBeCloseTo(264.5, 6)
  })

  it('не делит на ноль и не знает лишних функций', () => {
    expect(() => evaluateExpression('1 / 0')).toThrow('Деление на ноль')
    expect(() => evaluateExpression('foo(1)')).toThrow('Неизвестная функция')
    expect(() => evaluateExpression('2 +')).toThrow()
  })
})
