use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use super::node::lookup_failure;
use crate::app::Refresh;
use crate::app::api_client::{self, Answer, Line};
use crate::app::components::{EdgeList, NodeLink, StaleNote, TruncationNote, impact_href, loaded};

const DEPTHS: [u8; 5] = [1, 2, 3, 4, 5];

/// What depends on a node, level by level, to a chosen depth.
#[component]
pub fn Impact() -> impl IntoView {
    let refresh = expect_context::<Refresh>();
    let query = use_query_map();
    let path = Memo::new(move |_| query.read().get("path").unwrap_or_default());
    let depth = Memo::new(move |_| {
        query
            .read()
            .get("depth")
            .and_then(|depth| depth.parse::<u8>().ok())
            .filter(|depth| DEPTHS.contains(depth))
            .unwrap_or(2)
    });
    let answer = LocalResource::new(move || {
        refresh.track();
        let path = path.get();
        let depth = depth.get();
        async move { api_client::impact(&path, depth).await }
    });
    view! {
        <div class="page">
            <div class="page-head">
                <h1>"Impact"</h1>
                <div class="segmented" role="group" aria-label="Depth">
                    <span class="quiet">"Depth"</span>
                    {DEPTHS.into_iter().map(|value| view! {
                        <a
                            href=move || impact_href(&path.get(), value)
                            class:active=move || depth.get() == value
                        >
                            {value}
                        </a>
                    }).collect_view()}
                </div>
            </div>
            {loaded(answer, impact_view)}
        </div>
    }
}

fn impact_view(answer: Answer) -> AnyView {
    if let Some(failure) = lookup_failure(&answer) {
        return failure;
    }
    let mut target = None;
    let mut fields = Vec::new();
    for line in answer.section("impact") {
        match line {
            Line::Node(node) if target.is_none() => target = Some(node.clone()),
            Line::Field { key, value } => fields.push((key.clone(), value.clone())),
            _ => {}
        }
    }
    let mut levels: Vec<(u32, String)> = answer
        .sections
        .keys()
        .filter_map(|name| {
            name.strip_prefix("depth ")
                .and_then(|level| level.parse().ok())
                .map(|level| (level, name.clone()))
        })
        .collect();
    levels.sort();
    view! {
        {target.map(|node| view! { <div class="impact-target"><NodeLink node=node /></div> })}
        <dl class="fields">
            {fields.into_iter().map(|(key, value)| view! { <dt>{key}</dt><dd>{value}</dd> }).collect_view()}
        </dl>
        <StaleNote answer=answer.clone() />
        {levels.into_iter().map(|(level, name)| view! {
            <EdgeList title=format!("Depth {level}") edges=answer.edges(&name) hidden=answer.hidden(&name) show_empty=true />
        }).collect_view()}
        <TruncationNote answer=answer />
    }
    .into_any()
}
