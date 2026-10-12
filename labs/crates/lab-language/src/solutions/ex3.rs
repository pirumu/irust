use crate::ex3_dispatch::{Rect, Shape, Square};

pub fn total_area_static<S: Shape>(items: &[S]) -> u32 {
    items.iter().map(Shape::area).sum()
}

pub fn total_area_dyn(items: &[Box<dyn Shape>]) -> u32 {
    items.iter().map(|s| s.area()).sum()
}

pub fn largest(items: &[Box<dyn Shape>]) -> Option<&dyn Shape> {
    let mut best: Option<&dyn Shape> = None;
    for item in items {
        let item: &dyn Shape = &**item;
        match best {
            Some(b) if b.area() >= item.area() => {}
            _ => best = Some(item),
        }
    }
    best
}

pub fn make_shape(spec: &str) -> Option<Box<dyn Shape>> {
    let (kind, rest) = spec.split_once(':')?;
    match kind {
        "square" => Some(Box::new(Square(rest.parse().ok()?))),
        "rect" => {
            let (w, h) = rest.split_once('x')?;
            Some(Box::new(Rect(w.parse().ok()?, h.parse().ok()?)))
        }
        _ => None,
    }
}

pub fn apply_pipeline(stages: &[Box<dyn Fn(i32) -> i32>], x: i32) -> i32 {
    stages.iter().fold(x, |acc, f| f(acc))
}

pub fn compose<F, G>(f: F, g: G) -> impl Fn(i32) -> i32
where
    F: Fn(i32) -> i32,
    G: Fn(i32) -> i32,
{
    move |x| g(f(x))
}
