mod engine;
mod map;
mod run;
mod share;
mod tips;
mod ui;

use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    // a shared route shows a stored result and never starts the live app
    match share::route() {
        Some(id) => leptos::mount::mount_to_body(move || view! { <ui::Shared id=id/> }),
        None => leptos::mount::mount_to_body(ui::App),
    }
}
