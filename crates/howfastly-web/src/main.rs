mod engine;
mod map;
mod ui;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(ui::App);
}
