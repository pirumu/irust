use crate::ex2_trait::{Csv, Describe, JoinWith};
use std::fmt::{self, Display, Write as _};

impl<T: Display + ?Sized> Describe for T {
    fn describe(&self) -> String {
        format!("«{self}»")
    }
}

impl Display for Csv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.iter().join_with(","))
    }
}

impl<I: Iterator> JoinWith for I {
    fn join_with(self, sep: &str) -> String
    where
        Self: Sized,
        Self::Item: Display,
    {
        let mut out = String::new();
        for (i, item) in self.enumerate() {
            if i > 0 {
                out.push_str(sep);
            }
            write!(out, "{item}").expect("write! vào String không thất bại");
        }
        out
    }
}
