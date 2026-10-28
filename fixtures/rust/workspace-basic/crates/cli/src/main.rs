mod memory;

use fixture_core::geometry::{self, sides};
use fixture_core::{Area, Circle, MAX_SHAPES, Point, Shape, Wrapper, total_area};

fn config() -> usize {
    MAX_SHAPES / 2
}

fn shadowed() -> usize {
    let config = 3;
    config * 2
}

fn uses_global() -> usize {
    config() + 1
}

fn rebinds() -> usize {
    let config = config();
    config * geometry::sides(&Shape::Empty) as usize
}

fn main() {
    let circle = Circle::new(Point { x: 0.0, y: 0.0 }, 2.0);
    println!("{} {}", circle.describe(), sides(&Shape::Empty));
    let shapes: Vec<Box<dyn Area>> = vec![Box::new(circle)];
    let wrapped = Wrapper::<u32>::new(5);
    let sizes = fixture_core::parse_all::<u32>(&["1", "2"]);
    let sum = sizes.iter().fold(0, |acc, size| acc + size);
    println!("{} {} {}", total_area(&shapes), wrapped.get(), sum);
    println!("{}", shadowed() + uses_global());
    println!("{:?} {}", "7".parse::<u32>(), rebinds());
}
