mod card_matching;
mod formatters;
mod io;
mod models;
mod ui;
mod stock_analysis;

fn main() {
    if let Err(e) = ui::launch_gui() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
