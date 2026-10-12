pub fn read_then_push(v: &mut Vec<i32>) -> Option<i32> {
    let first = *v.first()?; // i32 là Copy: borrow chia sẻ kết thúc ngay ở đây
    v.push(first * 10);
    Some(first)
}

pub fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}

pub fn take_first(v: &mut [String]) -> Option<String> {
    v.first_mut().map(std::mem::take)
}

pub fn remove_empty(v: &mut Vec<String>) -> usize {
    let before = v.len();
    v.retain(|s| !s.is_empty());
    before - v.len()
}
