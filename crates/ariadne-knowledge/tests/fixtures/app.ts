/** Multiplies two numbers. */
export function multiply(a: number, b: number): number {
  return a * b;
}

export interface Scale {
  factor: number;
}

export class Scaler implements Scale {
  factor = 2;
  /** Scales a value. */
  scale(value: number): number {
    return multiply(value, this.factor);
  }
}

it('multiplies', () => {
  multiply(2, 3);
});
