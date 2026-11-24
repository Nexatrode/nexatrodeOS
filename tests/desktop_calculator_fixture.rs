//! Trusted local host-test fixture, not an application package format or ABI.
extern crate alloc;

#[path = "../experiments/slint-notes/src/calculator.rs"]
mod calculator;

use calculator::{Calculator, Command, Operation};
use std::io::{self, BufRead, Write};

fn main() {
    let mut calculator = Calculator::default();
    println!("READY");
    io::stdout().flush().expect("flush fixture greeting");
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line == "EXIT" { println!("BYE"); break; }
        if let Some(path) = line.strip_prefix("READ ") {
            match std::fs::read_to_string(path) {
                Ok(value) => println!("DATA {}", value.trim()),
                Err(error) => println!("READ_ERROR {:?}", error.kind()),
            }
        } else {
            let command = match line.as_str() {
                "CLEAR" => Some(Command::Clear),
                "=" => Some(Command::Equals),
                "+" => Some(Command::Operation(Operation::Add)),
                "-" => Some(Command::Operation(Operation::Subtract)),
                "*" => Some(Command::Operation(Operation::Multiply)),
                "/" => Some(Command::Operation(Operation::Divide)),
                _ if line.len() == 1 && line.as_bytes()[0].is_ascii_digit() => {
                    Some(Command::Digit(line.as_bytes()[0] - b'0'))
                }
                _ => None,
            };
            let display = match command {
                Some(command) => calculator.press(command),
                None => calculator.display(),
            };
            println!("DISPLAY {display}");
        }
        io::stdout().flush().expect("flush fixture response");
    }
}
