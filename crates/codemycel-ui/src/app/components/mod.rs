mod card;
mod edge_line;

use leptos::prelude::*;

use super::api_client::{self, Answer};

pub use card::CardView;
pub use edge_line::{EdgeList, NodeLink};

pub fn node_href(path: &str) -> String {
    format!("/node?path={}", api_client::encode(path))
}

pub fn impact_href(path: &str, depth: u8) -> String {
    format!("/impact?path={}&depth={depth}", api_client::encode(path))
}

pub fn community_href(path_or_id: &str) -> String {
    let id = path_or_id.strip_prefix("community:").unwrap_or(path_or_id);
    format!("/?community={}", api_client::encode(id))
}

/// `2026-10-29T09:25:56Z` as `2026-10-29 09:25 UTC`.
pub fn when(time: &str) -> String {
    match (time.get(..10), time.get(11..16)) {
        (Some(day), Some(clock)) => format!("{day} {clock} UTC"),
        _ => time.to_owned(),
    }
}

/// The last two segments of a community label, which tell communities apart on the map.
pub fn short_label(label: &str) -> String {
    let segments: Vec<&str> = label.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [.., parent, last] => format!("{parent}/{last}"),
        _ => label.to_owned(),
    }
}

#[component]
pub fn Message(#[prop(into)] text: String) -> impl IntoView {
    view! { <p class="message">{text}</p> }
}

#[component]
pub fn Heuristic() -> impl IntoView {
    view! {
        <span class="tag heuristic" title="Communities are computed by clustering verified edges; they are a heuristic, not a fact of the code.">
            "derived · heuristic"
        </span>
    }
}

/// Renders `resource` with `render` once it has loaded; quiet text while loading and the API's
/// message on failure.
pub fn loaded<T, V>(
    resource: LocalResource<Result<T, String>>,
    render: impl Fn(T) -> V + 'static,
) -> impl Fn() -> AnyView
where
    T: Clone + 'static,
    V: IntoView + 'static,
{
    move || match resource.get() {
        None => view! { <p class="quiet">"Loading…"</p> }.into_any(),
        Some(Err(error)) => view! { <Message text=error /> }.into_any(),
        Some(Ok(value)) => render(value).into_any(),
    }
}

/// The footer of a cut answer: which sections were cut and the tool's hint.
#[component]
pub fn TruncationNote(answer: Answer) -> impl IntoView {
    answer.truncated.map(|truncated| {
        let counts: Vec<String> = truncated
            .sections
            .iter()
            .map(|count| format!("{} of {} {}", count.shown, count.total, count.name))
            .collect();
        view! {
            <p class="quiet truncated">
                "Showing " {counts.join(", ")} " · " {truncated.hint}
            </p>
        }
    })
}

/// Stale files this answer shows facts from.
#[component]
pub fn StaleNote(answer: Answer) -> impl IntoView {
    let stale = answer.freshness.stale;
    (!stale.is_empty()).then(|| {
        view! {
            <p class="stale-note">
                <span class="tag stale">"stale"</span>
                " changed since indexing: " {stale.join(", ")}
            </p>
        }
    })
}
