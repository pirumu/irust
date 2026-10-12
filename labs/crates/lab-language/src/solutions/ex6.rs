use crate::ex6_iter::{Dedup, Pairs};

impl<I> Iterator for Dedup<I>
where
    I: Iterator,
    I::Item: PartialEq + Clone,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        loop {
            let x = self.inner.next()?;
            if self.last.as_ref() != Some(&x) {
                self.last = Some(x.clone());
                return Some(x);
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.inner.size_hint().1)
    }
}

impl<I> Iterator for Pairs<I>
where
    I: Iterator,
    I::Item: Clone,
{
    type Item = (I::Item, I::Item);

    fn next(&mut self) -> Option<Self::Item> {
        let prev = match self.prev.take() {
            Some(p) => p,
            None => self.inner.next()?,
        };
        let cur = self.inner.next()?;
        self.prev = Some(cur.clone());
        Some((prev, cur))
    }
}
