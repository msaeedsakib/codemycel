mod api_client;
mod components;
mod views;

use std::time::Duration;

use leptos::prelude::*;
use leptos_router::components::{A, Route, Router, Routes};
use leptos_router::hooks::{use_navigate, use_query_map};
use leptos_router::path;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use api_client::Meta;
use components::Message;

const POLL: Duration = Duration::from_secs(5);

/// Bumped whenever `/api/meta` reports a new `indexed_at`; every view's data depends on it, so
/// a sync made elsewhere re-queries what is on screen.
#[derive(Clone, Copy)]
pub(crate) struct Refresh(RwSignal<u32>);

impl Refresh {
    pub(crate) fn track(self) {
        self.0.track();
    }
}

/// Bumped when the colour scheme flips, so the canvas repaints in the new theme.
#[derive(Clone, Copy)]
pub(crate) struct ThemeChange(RwSignal<u32>);

impl ThemeChange {
    pub(crate) fn track(self) {
        self.0.track();
    }
}

pub fn mount() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let refresh = Refresh(RwSignal::new(0));
    let theme = ThemeChange(RwSignal::new(0));
    provide_context(refresh);
    provide_context(theme);
    watch_color_scheme(theme);

    let meta = RwSignal::new(None::<Result<Meta, String>>);
    let poll = move || {
        leptos::task::spawn_local(async move {
            let next = api_client::meta().await;
            let changed = meta.with_untracked(|previous| match (previous, &next) {
                (Some(Ok(previous)), Ok(next)) => previous.indexed_at != next.indexed_at,
                (Some(Err(_)), Ok(_)) => true,
                _ => false,
            });
            if meta.with_untracked(|previous| previous.as_ref() != Some(&next)) {
                meta.set(Some(next));
            }
            if changed {
                refresh.0.update(|version| *version += 1);
            }
        });
    };
    poll();
    set_interval(poll, POLL);

    view! {
        <Router>
            <Header meta=meta />
            <main>
                <Routes fallback=|| view! { <Message text="No page here." /> }>
                    <Route path=path!("/") view=views::community_map::CommunityMap />
                    <Route path=path!("/search") view=views::search::Search />
                    <Route path=path!("/node") view=views::node::NodePage />
                    <Route path=path!("/impact") view=views::impact::Impact />
                    <Route path=path!("/status") view=views::status::Status />
                    <Route path=path!("/notes") view=views::notes::Notes />
                </Routes>
            </main>
        </Router>
    }
}

fn watch_color_scheme(theme: ThemeChange) {
    let Some(query) = web_sys::window()
        .and_then(|window| window.match_media("(prefers-color-scheme: dark)").ok())
        .flatten()
    else {
        return;
    };
    let changed = Closure::<dyn Fn()>::new(move || theme.0.update(|version| *version += 1));
    if query
        .add_event_listener_with_callback("change", changed.as_ref().unchecked_ref())
        .is_ok()
    {
        changed.forget();
    }
}

#[component]
fn Header(meta: RwSignal<Option<Result<Meta, String>>>) -> impl IntoView {
    let refresh = expect_context::<Refresh>();
    let status = LocalResource::new(move || {
        refresh.track();
        api_client::status()
    });
    let repo = move || match meta.get() {
        Some(Ok(meta)) => meta.repo,
        Some(Err(error)) => error,
        None => String::new(),
    };
    let repo_name = move || {
        let repo = repo();
        repo.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let freshness = move || {
        status.get().map(|answer| match answer {
            Ok(answer) => {
                let indexed = answer
                    .freshness
                    .indexed_at
                    .as_deref()
                    .map(components::when)
                    .unwrap_or_default();
                let warn = answer.freshness.changed_files > 0;
                view! {
                    <span class="freshness" class:warn=warn title="Freshness of the index">
                        {answer.freshness.line()}
                        {(!indexed.is_empty()).then(|| format!(" · indexed {indexed}"))}
                    </span>
                }
                .into_any()
            }
            Err(error) => view! { <span class="freshness warn">{error}</span> }.into_any(),
        })
    };
    view! {
        <header class="top">
            <div class="top-row">
                <A href="/" attr:class="brand">
                    <span class="mark" aria-hidden="true"></span>
                    "codemycel"
                </A>
                <span class="repo" title=repo>{repo_name}</span>
                <nav>
                    <A href="/" exact=true>"Map"</A>
                    <A href="/search">"Search"</A>
                    <A href="/status">"Status"</A>
                    <A href="/notes">"Notes"</A>
                </nav>
                <SearchBox />
            </div>
            <div class="top-meta">{freshness}</div>
        </header>
    }
}

#[component]
fn SearchBox() -> impl IntoView {
    let navigate = use_navigate();
    let query = use_query_map();
    let input = NodeRef::<leptos::html::Input>::new();
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let Some(input) = input.get() else {
            return;
        };
        let text = input.value();
        if !text.trim().is_empty() {
            navigate(
                &format!("/search?q={}", api_client::encode(text.trim())),
                Default::default(),
            );
        }
    };
    view! {
        <form class="search-box" on:submit=submit role="search">
            <input
                node_ref=input
                type="search"
                placeholder="Search symbols"
                aria-label="Search symbols"
                prop:value=move || query.read().get("q").unwrap_or_default()
            />
        </form>
    }
}
