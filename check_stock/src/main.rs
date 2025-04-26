mod card_matching;
mod formatters;
mod io;
mod models;
mod ui;

fn main() {
    if let Err(e) = ui::launch_gui() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
