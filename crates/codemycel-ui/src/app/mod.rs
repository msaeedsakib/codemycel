use leptos::prelude::*;

pub fn mount() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(|| view! { <p>"codemycel"</p> });
}
