#[macro_export]
macro_rules! count_exprs {
    () => { 0usize };
    ($head:expr $(, $tail:expr)* $(,)?) => { 1usize + $crate::count_exprs!($($tail),*) };
}

#[macro_export]
macro_rules! hashmap {
    () => { ::std::collections::HashMap::new() };
    ($($k:expr => $v:expr),+ $(,)?) => {{
        let mut m = ::std::collections::HashMap::with_capacity($crate::count_exprs!($($k),+));
        $( m.insert($k, $v); )+
        m
    }};
}

#[macro_export]
macro_rules! max_of {
    ($x:expr $(,)?) => { $x };
    ($x:expr, $($rest:expr),+ $(,)?) => {{
        let a = $x;
        let b = $crate::max_of!($($rest),+);
        if a >= b { a } else { b }
    }};
}
