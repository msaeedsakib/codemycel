use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};

use crate::app::Refresh;
use crate::app::api_client::{self, Answer};
use crate::app::components::{CardView, Message, StaleNote, TruncationNote, loaded};

const KINDS: [(&str, &str); 11] = [
    ("", "All kinds"),
    ("function", "Functions"),
    ("method", "Methods"),
    ("struct", "Structs"),
    ("class", "Classes"),
    ("enum", "Enums"),
    ("trait", "Traits"),
    ("interface", "Interfaces"),
    ("type_alias", "Type aliases"),
    ("constant", "Constants"),
    ("file", "Files"),
];

/// Search with the tool's ranking; results as cards.
#[component]
pub fn Search() -> impl IntoView {
    let refresh = expect_context::<Refresh>();
    let query = use_query_map();
    let navigate = use_navigate();
    let text = Memo::new(move |_| query.read().get("q").unwrap_or_default());
    let kind = Memo::new(move |_| query.read().get("kind").unwrap_or_default());
    let results = LocalResource::new(move || {
        refresh.track();
        let text = text.get();
        let kind = kind.get();
        async move {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                api_client::search(&text, &kind).await.map(Some)
            }
        }
    });
    let choose_kind = move |event: leptos::ev::Event| {
        let kind = event_target_value(&event);
        navigate(
            &format!(
                "/search?q={}&kind={}",
                api_client::encode(&text.get_untracked()),
                api_client::encode(&kind)
            ),
            Default::default(),
        );
    };
    view! {
        <div class="page narrow">
            <div class="page-head">
                <h1>"Search"</h1>
                <select class="select" on:change=choose_kind aria-label="Kind">
                    {KINDS
                        .iter()
                        .map(|(value, label)| view! {
                            <option value=*value selected=move || kind.get() == *value>{*label}</option>
                        })
                        .collect_view()}
                </select>
            </div>
            {loaded(results, |answer: Option<Answer>| match answer {
                None => view! {
                    <Message text="Type a name, a path fragment or words from a doc comment in the search box above." />
                }
                .into_any(),
                Some(answer) => results_view(answer).into_any(),
            })}
        </div>
    }
}

fn results_view(answer: Answer) -> impl IntoView {
    let cards = answer.cards("results");
    let empty = answer
        .texts("results")
        .into_iter()
        .next()
        .unwrap_or_else(|| "No matches.".to_owned());
    let total = cards.len() + answer.hidden("results");
    view! {
        <StaleNote answer=answer.clone() />
        {if cards.is_empty() {
            view! { <Message text=empty /> }.into_any()
        } else {
            view! {
                <p class="quiet">{format!("{total} {}", if total == 1 { "match" } else { "matches" })}</p>
                <div class="cards">
                    {cards.into_iter().map(|card| view! { <CardView card=card link=true /> }).collect_view()}
                </div>
            }
            .into_any()
        }}
        <TruncationNote answer=answer />
    }
}
