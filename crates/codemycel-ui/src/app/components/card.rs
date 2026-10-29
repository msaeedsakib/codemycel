use leptos::prelude::*;

use super::{Heuristic, community_href, node_href};
use crate::app::api_client::Card;

/// A symbol card. `link` makes the name open the node page.
#[component]
pub fn CardView(card: Card, #[prop(optional)] link: bool) -> impl IntoView {
    let node = card.node.clone();
    let name = node.short_name().to_owned();
    let container = node.container().to_owned();
    let location = match (&node.file, node.lines()) {
        (Some(file), lines) if !lines.is_empty() => format!("{file}  {lines}"),
        (Some(file), _) => file.clone(),
        (None, _) => String::new(),
    };
    let counts = card.counts;
    let count_items: Vec<(&str, u64)> = [
        ("callers", counts.callers),
        ("callees", counts.callees),
        ("impls", counts.impls),
        ("overrides", counts.overrides),
        ("unresolved", u64::from(counts.unresolved)),
        ("external", u64::from(counts.external)),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .collect();
    let title = if link {
        view! { <a class="card-name" href=node_href(&node.path) title=node.path.clone()>{name}</a> }
            .into_any()
    } else {
        view! { <h1 class="card-name" title=node.path.clone()>{name}</h1> }.into_any()
    };
    view! {
        <article class="card">
            <div class="card-head">
                <span class="kind">{node.kind.replace('_', " ")}</span>
                {title}
                {card.visibility.map(|visibility| view! { <span class="tag">{visibility}</span> })}
            </div>
            {(!container.is_empty()).then(|| view! { <div class="card-path mono">{container}</div> })}
            {(!location.is_empty()).then(|| view! { <div class="card-location mono">{location}</div> })}
            {node.signature.map(|signature| view! { <pre class="signature">{signature}</pre> })}
            {card.doc.map(|doc| view! { <p class="doc">{doc}</p> })}
            <div class="card-foot">
                {card.community.map(|community| view! {
                    <a class="chip" href=community_href(&community.id) title=format!("community:{}", community.id)>
                        <span class="chip-dot"></span>
                        {community.label}
                    </a>
                    <Heuristic />
                })}
                <span class="counts">
                    {count_items
                        .into_iter()
                        .map(|(label, count)| view! { <span><b>{count}</b> " " {label}</span> })
                        .collect_view()}
                </span>
            </div>
            {(!card.notes.is_empty()).then(|| view! {
                <ul class="card-notes">
                    {card.notes.into_iter().map(|note| view! {
                        <li>
                            <span class="tag">{note.kind}</span>
                            " " {note.first_line}
                            <span class="quiet">" · " {note.updated.get(..10).unwrap_or_default().to_owned()}</span>
                        </li>
                    }).collect_view()}
                </ul>
            })}
        </article>
    }
}
