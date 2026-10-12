//! # Bài 6 — Tự viết iterator adapter
//!
//! Viết hai adapter như `map`/`filter` của std: một struct giữ trạng thái + `impl Iterator`,
//! rồi một *extension trait* (blanket impl) để gọi dạng method `.dedup_consecutive()`,
//! `.pairs()` trên **mọi** iterator.
//!
//! - [`Dedup`]: bỏ phần tử bằng phần tử **liền trước** (chỉ trùng liên tiếp, như `uniq`).
//! - [`Pairs`]: cặp liên tiếp chồng lấn: `1,2,3,4` → `(1,2),(2,3),(3,4)`.
//!
//! Yêu cầu: **lười** (không kéo phần tử nào của iterator gốc trước lần `next()` đầu), và
//! `Dedup` phải cài `size_hint` đúng: cận dưới `0`, cận trên bằng cận trên của iterator gốc.
//! Phần dựng struct và extension trait đã có sẵn; bạn viết hai `impl Iterator`.

pub struct Dedup<I: Iterator> {
    pub(crate) inner: I,
    pub(crate) last: Option<I::Item>,
}

pub struct Pairs<I: Iterator> {
    pub(crate) inner: I,
    pub(crate) prev: Option<I::Item>,
}

pub trait IterExt: Iterator + Sized {
    fn dedup_consecutive(self) -> Dedup<Self>
    where
        Self::Item: PartialEq + Clone,
    {
        Dedup {
            inner: self,
            last: None,
        }
    }

    fn pairs(self) -> Pairs<Self>
    where
        Self::Item: Clone,
    {
        Pairs {
            inner: self,
            prev: None,
        }
    }
}

impl<I: Iterator> IterExt for I {}

#[cfg(not(feature = "solution"))]
impl<I> Iterator for Dedup<I>
where
    I: Iterator,
    I::Item: PartialEq + Clone,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        todo!("loop: kéo từ inner; so với self.last; nhớ phần tử vừa trả")
    }

    // TODO: size_hint — mặc định (0, None) vẫn đúng nhưng test đòi cận trên chặt hơn.
}

#[cfg(not(feature = "solution"))]
impl<I> Iterator for Pairs<I>
where
    I: Iterator,
    I::Item: Clone,
{
    type Item = (I::Item, I::Item);

    fn next(&mut self) -> Option<Self::Item> {
        todo!("lần đầu kéo hai phần tử; các lần sau chỉ kéo thêm một")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn dedup_removes_only_consecutive_duplicates() {
        let got: Vec<i32> = [1, 1, 2, 2, 2, 3, 1, 1]
            .into_iter()
            .dedup_consecutive()
            .collect();
        assert_eq!(got, [1, 2, 3, 1]);
    }

    #[test]
    fn dedup_edge_cases() {
        assert_eq!(std::iter::empty::<i32>().dedup_consecutive().count(), 0);
        assert_eq!([7].into_iter().dedup_consecutive().collect::<Vec<_>>(), [7]);
        assert_eq!(
            [5, 5, 5]
                .into_iter()
                .dedup_consecutive()
                .collect::<Vec<_>>(),
            [5]
        );
        let words: Vec<&str> = "a a b b a".split(' ').dedup_consecutive().collect();
        assert_eq!(words, ["a", "b", "a"]);
    }

    #[test]
    fn dedup_is_lazy() {
        let pulled = Cell::new(0);
        let mut it = [1, 1, 2, 3]
            .into_iter()
            .inspect(|_| pulled.set(pulled.get() + 1))
            .dedup_consecutive();
        assert_eq!(pulled.get(), 0, "tạo adapter không được kéo phần tử nào");
        assert_eq!(it.next(), Some(1));
        assert_eq!(pulled.get(), 1);
        assert_eq!(it.next(), Some(2));
        assert_eq!(
            pulled.get(),
            3,
            "bỏ qua một phần tử trùng rồi dừng ở phần tử mới"
        );
    }

    #[test]
    fn dedup_size_hint_upper_bound_matches_inner() {
        let v = [1, 1, 2, 3, 3];
        let it = v.iter().dedup_consecutive();
        assert_eq!(it.size_hint(), (0, Some(5)));
    }

    #[test]
    fn pairs_overlapping() {
        let got: Vec<_> = [1, 2, 3, 4].into_iter().pairs().collect();
        assert_eq!(got, [(1, 2), (2, 3), (3, 4)]);
        let s: Vec<_> = "abc".chars().pairs().collect();
        assert_eq!(s, [('a', 'b'), ('b', 'c')]);
    }

    #[test]
    fn pairs_short_inputs_yield_nothing() {
        assert_eq!(std::iter::empty::<i32>().pairs().count(), 0);
        assert_eq!([1].into_iter().pairs().count(), 0);
    }

    #[test]
    fn adapters_compose_with_std_adapters() {
        // 1,1,2,2,5 → dedup 1,2,5 → pairs (1,2),(2,5) → hiệu 1,3
        let diffs: Vec<i32> = [1, 1, 2, 2, 5]
            .into_iter()
            .dedup_consecutive()
            .pairs()
            .map(|(a, b)| b - a)
            .collect();
        assert_eq!(diffs, [1, 3]);
        let total: i32 = (1..=4).pairs().map(|(a, b)| a * b).sum();
        assert_eq!(total, 2 + 6 + 12);
    }

    #[test]
    fn works_in_for_loop_and_by_ref() {
        let mut it = [1, 2, 2, 3].into_iter().dedup_consecutive();
        let mut seen = Vec::new();
        for x in it.by_ref().take(2) {
            seen.push(x);
        }
        assert_eq!(seen, [1, 2]);
        assert_eq!(it.next(), Some(3));
        assert_eq!(it.next(), None);
    }
}
