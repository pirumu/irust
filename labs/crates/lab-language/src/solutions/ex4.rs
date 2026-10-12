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
    _marker: PhantomData<RcBox<T>>,
}

impl<T> MyRc<T> {
    pub fn new(value: T) -> Self {
        let boxed = Box::new(RcBox {
            count: Cell::new(1),
            value,
        });
        MyRc {
            // Box::leak trả &mut không null, nên NonNull::from hợp lệ; quyền sở hữu giờ do
            // bộ đếm quản lý, và Drop sẽ lấy lại bằng Box::from_raw.
            ptr: NonNull::from(Box::leak(boxed)),
            _marker: PhantomData,
        }
    }

    fn inner(&self) -> &RcBox<T> {
        // SAFETY: bất biến — còn `self` thì RcBox còn sống (count >= 1), nên ptr hợp lệ;
        // ta chỉ tạo `&`, và mọi thay đổi đi qua `Cell`.
        unsafe { self.ptr.as_ref() }
    }

    pub fn strong_count(this: &Self) -> usize {
        this.inner().count.get()
    }

    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        a.ptr == b.ptr
    }
}

impl<T> Clone for MyRc<T> {
    fn clone(&self) -> Self {
        let inner = self.inner();
        let next = inner
            .count
            .get()
            .checked_add(1)
            .expect("MyRc: số tham chiếu tràn usize");
        inner.count.set(next);
        MyRc {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }
}

impl<T> Deref for MyRc<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner().value
    }
}

impl<T> Drop for MyRc<T> {
    fn drop(&mut self) {
        let remaining = self.inner().count.get() - 1;
        self.inner().count.set(remaining);
        if remaining == 0 {
            // SAFETY: đây là owner cuối (count vừa về 0) nên không còn MyRc nào đọc RcBox; ptr
            // đến từ Box::leak ở `new`, chưa từng bị giải phóng; from_raw trả lại quyền sở hữu
            // và drop RcBox (kéo theo drop T) đúng một lần.
            drop(unsafe { Box::from_raw(self.ptr.as_ptr()) });
        }
    }
}
