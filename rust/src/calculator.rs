//! Bounded integer calculator state for the future isolated GUI app.
//!
//! This module has no device or service authority. A GUI calculator can own
//! one instance in its protection domain and publish only the display string.

const DISPLAY_CAPACITY: usize = 20;
const INPUT_DIGIT_LIMIT: usize = 19;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Calculator {
    display: [u8; DISPLAY_CAPACITY],
    length: usize,
    accumulator: Option<i64>,
    pending: Option<Operation>,
    replace_on_digit: bool,
    error: bool,
}

impl Calculator {
    pub const fn new() -> Self {
        let mut display = [0; DISPLAY_CAPACITY];
        display[0] = b'0';
        Self {
            display,
            length: 1,
            accumulator: None,
            pending: None,
            replace_on_digit: false,
            error: false,
        }
    }

    pub fn display(&self) -> &[u8] {
        &self.display[..self.length]
    }

    pub fn press_digit(&mut self, digit: u8) -> bool {
        if digit > 9 {
            return false;
        }
        if self.error {
            self.clear();
        }
        if self.replace_on_digit {
            self.display.fill(0);
            self.length = 0;
            self.replace_on_digit = false;
        }
        if self.display() == b"0" {
            if digit == 0 {
                return false;
            }
            self.display[0] = b'0' + digit;
            return true;
        }
        if self.length >= INPUT_DIGIT_LIMIT {
            return false;
        }
        self.display[self.length] = b'0' + digit;
        self.length += 1;
        true
    }

    pub fn press_operation(&mut self, operation: Operation) -> bool {
        if self.error {
            return false;
        }
        let value = match self.pending {
            Some(previous) if !self.replace_on_digit => {
                let Some(left) = self.accumulator else {
                    return self.fail();
                };
                let Some(right) = self.parse_display() else {
                    return self.fail();
                };
                let Some(result) = apply(previous, left, right) else {
                    return self.fail();
                };
                result
            }
            _ => {
                let Some(value) = self.parse_display() else {
                    return self.fail();
                };
                value
            }
        };
        self.accumulator = Some(value);
        self.pending = Some(operation);
        self.replace_on_digit = true;
        true
    }

    pub fn equals(&mut self) -> bool {
        if self.error {
            return false;
        }
        let (Some(operation), Some(left)) = (self.pending, self.accumulator) else {
            return false;
        };
        let Some(right) = self.parse_display() else {
            return self.fail();
        };
        let Some(result) = apply(operation, left, right) else {
            return self.fail();
        };
        self.set_display(result);
        self.accumulator = None;
        self.pending = None;
        self.replace_on_digit = true;
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.error {
            self.clear();
            return true;
        }
        if (self.replace_on_digit && self.pending.is_some()) || self.length == 1 {
            return false;
        }
        self.replace_on_digit = false;
        if self.length == 2 && self.display[0] == b'-' {
            self.display.fill(0);
            self.display[0] = b'0';
            self.length = 1;
            return true;
        }
        self.length -= 1;
        self.display[self.length] = 0;
        true
    }

    pub fn clear(&mut self) {
        self.display.fill(0);
        self.display[0] = b'0';
        self.length = 1;
        self.accumulator = None;
        self.pending = None;
        self.replace_on_digit = false;
        self.error = false;
    }

    fn parse_display(&self) -> Option<i64> {
        let (negative, digits) = if self.display[0] == b'-' {
            (true, &self.display[1..self.length])
        } else {
            (false, &self.display[..self.length])
        };
        let mut magnitude = 0u64;
        for digit in digits {
            magnitude = magnitude
                .checked_mul(10)?
                .checked_add((digit - b'0') as u64)?;
        }
        if negative {
            if magnitude == (i64::MAX as u64) + 1 {
                Some(i64::MIN)
            } else {
                i64::try_from(magnitude).ok().map(|value| -value)
            }
        } else {
            i64::try_from(magnitude).ok()
        }
    }

    fn set_display(&mut self, value: i64) {
        self.display.fill(0);
        let negative = value < 0;
        let mut magnitude = value.unsigned_abs();
        let mut reversed = [0u8; DISPLAY_CAPACITY];
        let mut digits = 0;
        loop {
            reversed[digits] = b'0' + (magnitude % 10) as u8;
            digits += 1;
            magnitude /= 10;
            if magnitude == 0 {
                break;
            }
        }
        let sign_length = usize::from(negative);
        if negative {
            self.display[0] = b'-';
        }
        for index in 0..digits {
            self.display[sign_length + index] = reversed[digits - index - 1];
        }
        self.length = sign_length + digits;
    }

    fn fail(&mut self) -> bool {
        self.display.fill(0);
        self.display[..5].copy_from_slice(b"ERROR");
        self.length = 5;
        self.accumulator = None;
        self.pending = None;
        self.replace_on_digit = true;
        self.error = true;
        true
    }
}

impl Default for Calculator {
    fn default() -> Self {
        Self::new()
    }
}

fn apply(operation: Operation, left: i64, right: i64) -> Option<i64> {
    match operation {
        Operation::Add => left.checked_add(right),
        Operation::Subtract => left.checked_sub(right),
        Operation::Multiply => left.checked_mul(right),
        Operation::Divide => left.checked_div(right),
    }
}

#[cfg(test)]
mod tests {
    use super::{Calculator, Operation};

    fn enter(calculator: &mut Calculator, value: &str) {
        for digit in value.bytes() {
            assert!(calculator.press_digit(digit - b'0'));
        }
    }

    #[test]
    fn performs_chained_checked_integer_operations() {
        let mut calculator = Calculator::new();
        enter(&mut calculator, "12");
        assert!(calculator.press_operation(Operation::Add));
        enter(&mut calculator, "3");
        assert!(calculator.press_operation(Operation::Multiply));
        enter(&mut calculator, "4");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"60");
    }

    #[test]
    fn supports_negative_results_and_checked_overflow() {
        let mut calculator = Calculator::new();
        enter(&mut calculator, "2");
        assert!(calculator.press_operation(Operation::Subtract));
        enter(&mut calculator, "5");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"-3");

        calculator.clear();
        enter(&mut calculator, "9223372036854775807");
        assert!(calculator.press_operation(Operation::Add));
        enter(&mut calculator, "1");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"ERROR");
    }

    #[test]
    fn formats_and_reuses_the_minimum_integer() {
        let mut calculator = Calculator::new();
        assert!(calculator.press_operation(Operation::Subtract));
        enter(&mut calculator, "9223372036854775807");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"-9223372036854775807");
        assert!(calculator.press_operation(Operation::Subtract));
        enter(&mut calculator, "1");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"-9223372036854775808");
        assert!(calculator.press_operation(Operation::Add));
        enter(&mut calculator, "0");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"-9223372036854775808");
    }

    #[test]
    fn backspace_clears_a_one_digit_negative_result_safely() {
        let mut calculator = Calculator::new();
        enter(&mut calculator, "2");
        assert!(calculator.press_operation(Operation::Subtract));
        enter(&mut calculator, "3");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"-1");
        assert!(calculator.backspace());
        assert_eq!(calculator.display(), b"0");
    }

    #[test]
    fn division_by_zero_and_overflow_are_visible_errors() {
        let mut calculator = Calculator::new();
        enter(&mut calculator, "9");
        assert!(calculator.press_operation(Operation::Divide));
        enter(&mut calculator, "0");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"ERROR");

        assert!(calculator.press_digit(2));
        assert_eq!(calculator.display(), b"2");
        assert!(calculator.press_operation(Operation::Subtract));
        enter(&mut calculator, "3");
        assert!(calculator.equals());
        assert_eq!(calculator.display(), b"-1");
    }

    #[test]
    fn rejects_invalid_digits_and_bounds_input() {
        let mut calculator = Calculator::new();
        assert!(!calculator.press_digit(10));
        for digit in b"12345678901234567890" {
            calculator.press_digit(digit - b'0');
        }
        assert_eq!(calculator.display().len(), 19);
        assert!(!calculator.press_digit(1));
    }
}
