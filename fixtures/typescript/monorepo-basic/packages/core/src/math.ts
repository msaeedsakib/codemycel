export const TAU = Math.PI * 2;
let calls = 0;
const config = { precision: 2 };

/**
 * Rounds a value to the configured precision.
 */
export function round(value: number): number {
  calls += 1;
  const factor = 10 ** config.precision;
  return Math.round(value * factor) / factor;
}

export function scale(value: number, by: number): number;
export function scale(value: string, by: number): string;
export function scale(value: number | string, by: number): number | string {
  return typeof value === "number" ? round(value * by) : value.repeat(by);
}

export const double = (value: number): number => scale(value, 2);

export const half = (value: number) => value / 2;

export function sum(values: number[]) {
  let total = 0;
  for (const value of values) {
    total += value;
  }
  return total;
}

export function withLocalConfig(): number {
  const config = { precision: 0 };
  return config.precision + calls;
}

export namespace Units {
  export const meter = 1;

  export function toCentimeters(meters: number): number {
    return meters * 100 * meter;
  }
}

const square = function (value: number) {
  return value * value;
};

export { square, config as settings };
