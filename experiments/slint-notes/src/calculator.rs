//! Calculator state used by the Slint suite, independent of UI and transport.
//!
//! These Rust types are an internal interface, not a stable cross-process ABI.

use alloc::string::{String, ToString};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Digit(u8),
    Operation(Operation),
    Equals,
    Clear,
}

#[derive(Default)]
pub struct Calculator {
    entry: i64,
    accumulator: Option<i64>,
    operation: Option<Operation>,
    replace_entry: bool,
    error: bool,
}

impl Calculator {
    pub fn display(&self) -> String {
        if self.error {
            String::from("ERROR")
        } else {
            self.entry.to_string()
        }
    }

    pub fn press(&mut self, command: Command) -> String {
        match command {
            Command::Clear => *self = Self::default(),
            Command::Digit(digit) if digit <= 9 => {
                if self.error {
                    *self = Self::default();
                }
                let next = if self.replace_entry {
                    Some(digit as i64)
                } else {
                    self.entry
                        .checked_mul(10)
                        .and_then(|value| value.checked_add(digit as i64))
                };
                if let Some(value) = next.filter(|value| *value <= 999_999_999) {
                    self.entry = value;
                    self.replace_entry = false;
                }
            }
            Command::Digit(_) => {}
            Command::Operation(_) | Command::Equals if self.error => {}
            Command::Operation(_) | Command::Equals => {
                if let (Some(left), Some(op)) = (self.accumulator, self.operation) {
                    let result = match op {
                        Operation::Add => left.checked_add(self.entry),
                        Operation::Subtract => left.checked_sub(self.entry),
                        Operation::Multiply => left.checked_mul(self.entry),
                        Operation::Divide => left.checked_div(self.entry),
                    };
                    let Some(result) = result else {
                        self.error = true;
                        return self.display();
                    };
                    self.entry = result;
                }
                match command {
                    Command::Operation(op) => {
                        self.accumulator = Some(self.entry);
                        self.operation = Some(op);
                    }
                    Command::Equals => {
                        self.accumulator = None;
                        self.operation = None;
                    }
                    _ => unreachable!(),
                }
                self.replace_entry = true;
            }
        }
        self.display()
    }
}

#[cfg(test)]
mod tests {
    use super::{Calculator, Command, Operation};

    #[test]
    fn arithmetic_and_recovery_are_independent_of_the_ui() {
        let mut calc = Calculator::default();
        for command in [
            Command::Digit(7),
            Command::Operation(Operation::Add),
            Command::Digit(5),
        ] {
            calc.press(command);
        }
        assert_eq!(calc.press(Command::Equals), "12");
        for command in [
            Command::Clear,
            Command::Digit(9),
            Command::Operation(Operation::Divide),
            Command::Digit(0),
        ] {
            calc.press(command);
        }
        assert_eq!(calc.press(Command::Equals), "ERROR");
        assert_eq!(calc.press(Command::Digit(3)), "3");
    }

    #[test]
    fn invalid_and_overlong_digit_input_is_bounded() {
        let mut calc = Calculator::default();
        assert_eq!(calc.press(Command::Digit(10)), "0");
        for _ in 0..9 {
            calc.press(Command::Digit(9));
        }
        assert_eq!(calc.press(Command::Digit(9)), "999999999");
    }

    #[test]
    fn pending_operations_and_clear_keep_existing_semantics() {
        let mut calc = Calculator::default();
        for command in [
            Command::Digit(7),
            Command::Operation(Operation::Subtract),
            Command::Digit(9),
            Command::Equals,
        ] {
            calc.press(command);
        }
        assert_eq!(calc.display(), "-2");
        assert_eq!(calc.press(Command::Clear), "0");
    }
}
