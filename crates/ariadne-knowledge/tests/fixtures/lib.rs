//! The Rust fixture.

/// Adds two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// A counter.
pub struct Counter {
    total: i32,
}

impl Counter {
    /// Counts one more.
    pub fn bump(&mut self) {
        self.total = add(self.total, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(1, 2), 3);
    }
}
