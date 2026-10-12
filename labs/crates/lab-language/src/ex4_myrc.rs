//! # Bài 4 — Tự viết `MyRc<T>`: smart pointer đếm tham chiếu
//!
//! Mục tiêu: tự tay nối `Deref`, `Clone` và `Drop` thành một con trỏ nhiều owner, với bộ đếm
//! nằm trong `Cell<usize>` (interior mutability: `clone(&self)` chỉ có `&self` mà vẫn tăng đếm).
//!
//! Bố cục bộ nhớ gợi ý (đã có sẵn trong stub):
//!
//! ```text
//! MyRc<T> { ptr } ──► RcBox<T> { count: Cell<usize>, value: T }   (cấp phát heap, MỘT bản)
//! ```
//!
//! - `new`: cấp phát `RcBox` với `count = 1`. Cần một con trỏ thô tới heap:
//!   `Box::new(..)` rồi `Box::leak(..)` (hoặc `Box::into_raw`), bọc bằng `NonNull`.
//! - `Clone`: tăng đếm, trả `MyRc` mới trỏ **cùng** `RcBox` (không copy `T`).
//! - `Deref`: trả `&T` trỏ vào `value`.
//! - `Drop`: giảm đếm; khi về 0 thì trả bộ nhớ bằng `Box::from_raw` (drop đúng **một** lần).
//!
//! Phần `unsafe` tối thiểu và có bất biến rõ ràng: *khi còn bất kỳ `MyRc` nào, `ptr` trỏ vào một
//! `RcBox` còn sống; `RcBox` chỉ bị giải phóng khi `MyRc` cuối cùng bị drop*. Mỗi khối `unsafe`
//! bạn viết hãy ghi `// SAFETY:` nêu bất biến nào đảm bảo nó.
//!
//! `MyRc<T>` phải **không** `Send` (đếm bằng `Cell` không an toàn khi nhiều thread). Với
//! `NonNull<..>` làm trường, trình biên dịch tự suy ra điều đó:
//!
//! ```compile_fail,E0277
//! use lab_language::ex4_myrc::MyRc;
//! let a = MyRc::new(1);
//! std::thread::spawn(move || {
//!     let _a = a;
//! });
//! ```
//!
//! Nâng cao (không có test): chạy test dưới Miri nếu máy bạn có (`cargo +nightly miri test`),
//! thêm `Weak`, `get_mut`, `try_unwrap`.

#[cfg(not(feature = "solution"))]
mod stub {
    use std::cell::Cell;
    use std::marker::PhantomData;
    use std::ops::Deref;
    use std::ptr::NonNull;

    struct RcBox<T> {
        count: Cell<usize>,
        value: T,
    }

    pub struct MyRc<T> {
        ptr: NonNull<RcBox<T>>,
        // báo cho dropck rằng MyRc "sở hữu" một RcBox<T>
        _marker: PhantomData<RcBox<T>>,
    }

    impl<T> MyRc<T> {
        pub fn new(value: T) -> Self {
            todo!("cấp phát RcBox với count = 1 và value")
        }

        /// Số `MyRc` đang trỏ cùng một giá trị (hàm liên kết, như `Rc::strong_count`).
        pub fn strong_count(this: &Self) -> usize {
            todo!()
        }

        /// Hai `MyRc` có trỏ cùng một `RcBox` không (như `Rc::ptr_eq`).
        pub fn ptr_eq(a: &Self, b: &Self) -> bool {
            todo!()
        }
    }

    impl<T> Clone for MyRc<T> {
        fn clone(&self) -> Self {
            todo!("tăng count")
        }
    }

    impl<T> Deref for MyRc<T> {
        type Target = T;
        fn deref(&self) -> &T {
            todo!()
        }
    }

    // TODO: `impl<T> Drop for MyRc<T>` — giảm đếm, về 0 thì giải phóng.
}

#[cfg(not(feature = "solution"))]
pub use stub::MyRc;

#[cfg(feature = "solution")]
pub use crate::solutions::ex4::MyRc;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// Đếm số lần `drop` chạy — bên quan sát dùng `std::rc::Rc`, chỉ trong test.
    struct DropCounter(Rc<Cell<u32>>);
    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn count_follows_clone_and_drop() {
        let a = MyRc::new(5);
        assert_eq!(MyRc::strong_count(&a), 1);
        let b = a.clone();
        assert_eq!(MyRc::strong_count(&a), 2);
        assert_eq!(MyRc::strong_count(&b), 2);
        drop(b);
        assert_eq!(MyRc::strong_count(&a), 1);
    }

    #[test]
    fn deref_gives_access_to_inner() {
        let a = MyRc::new(String::from("abc"));
        assert_eq!(*a, "abc");
        assert_eq!(a.len(), 3); // auto-deref tới String::len
    }

    #[test]
    fn clones_share_one_allocation() {
        let a = MyRc::new(vec![1, 2, 3]);
        let b = a.clone();
        assert!(MyRc::ptr_eq(&a, &b));
        assert!(std::ptr::eq(&*a, &*b));
        let other = MyRc::new(vec![1, 2, 3]);
        assert!(!MyRc::ptr_eq(&a, &other));
    }

    #[test]
    fn inner_dropped_exactly_once_when_last_owner_goes() {
        let drops = Rc::new(Cell::new(0));
        let a = MyRc::new(DropCounter(drops.clone()));
        let b = a.clone();
        let c = b.clone();
        assert_eq!(MyRc::strong_count(&a), 3);
        drop(a);
        drop(b);
        assert_eq!(drops.get(), 0, "còn một owner thì chưa được drop T");
        drop(c);
        assert_eq!(drops.get(), 1, "owner cuối đi thì drop T đúng một lần");
    }

    #[test]
    fn works_with_interior_mutability_inside() {
        let log = MyRc::new(RefCell::new(Vec::<i32>::new()));
        let writer = log.clone();
        writer.borrow_mut().push(1);
        log.borrow_mut().push(2);
        assert_eq!(*log.borrow(), [1, 2]);
        assert_eq!(MyRc::strong_count(&log), 2);
    }

    #[test]
    fn many_clones_in_a_collection() {
        let drops = Rc::new(Cell::new(0));
        let root = MyRc::new(DropCounter(drops.clone()));
        let all: Vec<_> = (0..100).map(|_| root.clone()).collect();
        assert_eq!(MyRc::strong_count(&root), 101);
        drop(all);
        assert_eq!(MyRc::strong_count(&root), 1);
        assert_eq!(drops.get(), 0);
        drop(root);
        assert_eq!(drops.get(), 1);
    }
}
