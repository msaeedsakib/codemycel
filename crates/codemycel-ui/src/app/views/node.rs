use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::app::Refresh;
use crate::app::api_client::{self, Answer, Line};
use crate::app::components::{
    CardView, EdgeList, Message, NodeLink, StaleNote, TruncationNote, impact_href, loaded,
};

/// The node page: card, relations, unresolved candidates, notes and the body from disk.
#[component]
pub fn NodePage() -> impl IntoView {
    let refresh = expect_context::<Refresh>();
    let query = use_query_map();
    let path = Memo::new(move |_| query.read().get("path").unwrap_or_default());
    let answer = LocalResource::new(move || {
        refresh.track();
        let path = path.get();
        async move { api_client::symbol(&path).await }
    });
    view! { <div class="page">{loaded(answer, node_view)}</div> }
}

/// What a lookup answered instead of a node: a not-found message or the candidates of an
/// ambiguous name. `None` when the answer is a node.
pub fn lookup_failure(answer: &Answer) -> Option<AnyView> {
    match answer.outcome.as_str() {
        "ambiguous" => {
            let candidates: Vec<_> = answer
                .section("candidates")
                .iter()
                .filter_map(|line| match line {
                    Line::Node(node) => Some(node.clone()),
                    _ => None,
                })
                .collect();
            Some(
                view! {
                    <div class="narrow">
                        <h1>"Several symbols match"</h1>
                        <ul class="plain-list">
                            {candidates.into_iter().map(|node| view! { <li><NodeLink node=node /></li> }).collect_view()}
                        </ul>
                    </div>
                }
                .into_any(),
            )
        }
        "not_found" => {
            let text = answer
                .texts("result")
                .into_iter()
                .next()
                .unwrap_or_else(|| "Nothing matches.".to_owned());
            Some(view! { <Message text=text /> }.into_any())
        }
        _ => None,
    }
}

fn node_view(answer: Answer) -> AnyView {
    if let Some(failure) = lookup_failure(&answer) {
        return failure;
    }
    let Some(card) = answer.cards("symbol").into_iter().next() else {
        return view! { <Message text="This answer has no symbol card." /> }.into_any();
    };
    let path = card.node.path.clone();
    let file = card.node.file.clone();
    let unresolved: Vec<(String, String)> = answer
        .section("unresolved")
        .iter()
        .filter_map(|line| match line {
            Line::Unresolved(line) => Some((
                format!("{}:{}", line.file, line.line),
                format!("{} · {}", line.text, line.reason.replace('_', " ")),
            )),
            _ => None,
        })
        .collect();
    let notes: Vec<_> = answer
        .section("notes")
        .iter()
        .filter_map(|line| match line {
            Line::Note(note) => Some(note.clone()),
            _ => None,
        })
        .collect();
    view! {
        <div class="node-top">
            <CardView card=card />
            <a class="button" href=impact_href(&path, 2)>"Impact"</a>
        </div>
        <StaleNote answer=answer.clone() />
        <div class="columns">
            <EdgeList title="Callers" edges=answer.edges("callers") hidden=answer.hidden("callers") show_empty=true />
            <EdgeList title="Callees" edges=answer.edges("callees") hidden=answer.hidden("callees") show_empty=true />
        </div>
        <div class="columns">
            <EdgeList title="Implementers" edges=answer.edges("implementers") hidden=answer.hidden("implementers") />
            <EdgeList title="Implements" edges=answer.edges("implements") hidden=answer.hidden("implements") />
            <EdgeList title="Overrides" edges=answer.edges("overrides") hidden=answer.hidden("overrides") />
        </div>
        {(!notes.is_empty()).then(|| view! {
            <section class="edges">
                <h2>"Notes " <span class="count">{notes.len()}</span></h2>
                {notes.into_iter().map(|note| view! {
                    <div class="note">
                        <div class="note-head">
                            <span class="tag">{note.kind}</span>
                            <span class="quiet">{note.updated.get(..10).unwrap_or_default().to_owned()}</span>
                        </div>
                        <div class="note-body">{note.body}</div>
                    </div>
                }).collect_view()}
            </section>
        })}
        <Body answer=answer.clone() file=file />
        {(!unresolved.is_empty()).then(|| {
            let count = unresolved.len() + answer.hidden("unresolved");
            view! {
                <details class="unresolved">
                    <summary>"Unresolved candidates " <span class="count">{count}</span></summary>
                    <ul>
                        {unresolved.into_iter().map(|(at, what)| view! {
                            <li><span class="mono where">{at}</span> " " <span class="mono">{what}</span></li>
                        }).collect_view()}
                    </ul>
                </details>
            }
        })}
        <TruncationNote answer=answer />
    }
    .into_any()
}

/// The body as on disk, line-numbered, with the stale marker when the file changed since
/// indexing.
#[component]
fn Body(answer: Answer, file: Option<String>) -> impl IntoView {
    let lines = answer.section("body").to_vec();
    if lines.is_empty() {
        return None;
    }
    let stale = lines.iter().find_map(|line| match line {
        Line::Field { key, value } if key == "stale" => Some(value.clone()),
        _ => None,
    });
    let notes: Vec<String> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let source: Vec<(u32, String)> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Source(source) => Some((source.line, source.text.clone())),
            _ => None,
        })
        .collect();
    Some(view! {
        <section class="body">
            <h2>
                "Body"
                {file.map(|file| view! { <span class="mono where">{file}</span> })}
                {stale.is_some().then(|| view! { <span class="tag stale">"stale"</span> })}
            </h2>
            {stale.map(|reason| view! { <p class="stale-note">{reason}</p> })}
            {notes.into_iter().map(|text| view! { <p class="quiet">{text}</p> }).collect_view()}
            {(!source.is_empty()).then(|| view! {
                <pre class="code"><code>
                    {source.into_iter().map(|(number, text)| view! {
                        <span class="code-line"><span class="ln">{number}</span>{text}"\n"</span>
                    }).collect_view()}
                </code></pre>
            })}
        </section>
    })
}
