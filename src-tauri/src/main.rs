// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  rams_lib::run();
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_main_logic_entry() {
        // This test ensures that the main crate can link to the library correctly.
        assert!(true);
    }
}
