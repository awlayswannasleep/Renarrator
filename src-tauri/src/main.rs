// Точка входа Renarrator.
// В release-сборке консоль не создаётся (приложение фоновое, живёт в трее).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    renarrator_lib::run()
}
