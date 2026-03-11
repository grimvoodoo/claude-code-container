mod api;
mod components;

use components::app::App;

fn main() {
    dioxus::launch(App);
}
