export interface Shape {
  readonly name: string;
  area(): number;
}

export abstract class Base {
  abstract readonly name: string;
  abstract area(): number;

  describe(): string {
    return `${this.name}: ${this.area()}`;
  }
}

export class Circle extends Base implements Shape {
  readonly name = "circle";

  constructor(private readonly radius: number) {
    super();
  }

  area(): number {
    return Math.PI * this.radius ** 2;
  }
}

export class Square extends Base implements Shape {
  readonly name = "square";
  side: number;

  constructor(side: number) {
    super();
    this.side = side;
  }

  area() {
    return this.side * this.side;
  }
}

export enum Kind {
  Circle,
  Square = "square",
}
