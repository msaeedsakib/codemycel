#[cfg(target_arch = "wasm32")]
fn main() {
    codemycel_ui::app::mount();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
