use leptos::prelude::*;

use super::node_href;
use crate::app::api_client::{EdgeLine, NodeLine};

/// A node's name linking to its page, with kind and location beside it.
#[component]
pub fn NodeLink(node: NodeLine) -> impl IntoView {
    let location = match (&node.file, node.lines()) {
        (Some(file), lines) if !lines.is_empty() => format!("{file}  {lines}"),
        (Some(file), _) => file.clone(),
        (None, _) => String::new(),
    };
    view! {
        <a class="node-link" href=node_href(&node.path) title=node.path.clone()>
            {node.short_name().to_owned()}
        </a>
        <span class="kind small">{node.kind.replace('_', " ")}</span>
        <span class="where mono">{location}</span>
    }
}

/// The edges of one section, or nothing when there are none. `hidden` counts lines the
/// budget cut.
#[component]
pub fn EdgeList(
    #[prop(into)] title: String,
    edges: Vec<EdgeLine>,
    #[prop(optional)] hidden: usize,
    #[prop(optional)] show_empty: bool,
) -> impl IntoView {
    if edges.is_empty() && !show_empty {
        return None;
    }
    let mixed = edges
        .iter()
        .any(|edge| edge.verified_by != edges[0].verified_by);
    let count = edges.len() + hidden;
    Some(view! {
        <section class="edges">
            <h2>{title} <span class="count">{count}</span></h2>
            {edges.is_empty().then(|| view! { <p class="quiet">"None"</p> })}
            <ul>
                {edges.into_iter().map(|edge| {
                    let arrow = if edge.direction == "in" { "←" } else { "→" };
                    let sites = (!edge.sites.is_empty()).then(|| {
                        let sites: Vec<String> = edge.sites.iter().map(|line| format!("L{line}")).collect();
                        view! { <span class="sites mono">{format!("[{}]", sites.join(", "))}</span> }
                    });
                    let syntactic = edge.verified_by == "syntactic";
                    let verified = (mixed && !syntactic).then(|| {
                        view! { <span class="tag">{format!("verified: {}", edge.verified_by)}</span> }
                    });
                    view! {
                        <li class="edge" class:syntactic=syntactic>
                            <span class="arrow" aria-hidden="true">{arrow}</span>
                            <span class="edge-kind">{edge.kind.replace('_', " ")}</span>
                            <NodeLink node=edge.node />
                            {sites}
                            {verified}
                            {syntactic.then(|| view! { <span class="tag">"syntactic"</span> })}
                            {edge.via.map(|via| view! {
                                <span class="via">"via " <a href=node_href(&via) title=via.clone()>{short(&via)}</a></span>
                            })}
                        </li>
                    }
                }).collect_view()}
            </ul>
            {(hidden > 0).then(|| view! { <p class="quiet">{format!("{hidden} more not shown")}</p> })}
        </section>
    })
}

fn short(path: &str) -> String {
    path.rsplit_once("::")
        .map_or(path, |(_, tail)| tail)
        .to_owned()
}
