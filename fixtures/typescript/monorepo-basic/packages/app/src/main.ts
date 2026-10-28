import { Circle, Square, double, type Shape } from "@fixture/core";
import * as core from "@fixture/core";
import "./setup";

export class Gallery {
  private readonly shapes: Shape[] = [];

  add = (shape: Shape): this => {
    this.shapes.push(shape);
    return this;
  };

  total(): number {
    return core.sum(this.shapes.map((shape) => shape.area()));
  }
}

export function build(): Gallery {
  const gallery = new Gallery();
  gallery.add(new Circle(1)).add(new Square(double(2)));
  return gallery;
}
