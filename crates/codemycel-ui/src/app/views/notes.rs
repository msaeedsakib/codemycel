use leptos::prelude::*;

use crate::app::Refresh;
use crate::app::api_client::{self, Answer, Line};
use crate::app::components::{
    Heuristic, Message, TruncationNote, community_href, loaded, node_href,
};

/// Every note agents attached to the repository, its symbols, files and communities.
#[component]
pub fn Notes() -> impl IntoView {
    let refresh = expect_context::<Refresh>();
    let answer = LocalResource::new(move || {
        refresh.track();
        api_client::notes()
    });
    view! {
        <div class="page narrow">
            <div class="page-head"><h1>"Notes"</h1></div>
            {loaded(answer, notes_view)}
        </div>
    }
}

fn notes_view(answer: Answer) -> impl IntoView {
    let notes: Vec<_> = answer
        .section("notes")
        .iter()
        .filter_map(|line| match line {
            Line::Note(note) => Some(note.clone()),
            _ => None,
        })
        .collect();
    view! {
        {notes.is_empty().then(|| view! { <Message text="No notes yet." /> })}
        <div class="cards">
            {notes.into_iter().map(|note| {
                let target = if note.target == "repo" {
                    view! { <span class="node-link">"repository"</span> }.into_any()
                } else if note.derived {
                    view! {
                        <a class="node-link" href=community_href(&note.target)>{note.target.clone()}</a>
                        <Heuristic />
                    }
                    .into_any()
                } else {
                    view! { <a class="node-link mono" href=node_href(&note.target)>{note.target.clone()}</a> }.into_any()
                };
                view! {
                    <article class="note card">
                        <div class="note-head">
                            <span class="tag">{note.kind}</span>
                            {target}
                            {note.orphaned.then(|| view! { <span class="tag warn">"orphaned"</span> })}
                            <span class="quiet push">{note.updated.get(..10).unwrap_or_default().to_owned()}</span>
                        </div>
                        <div class="note-body">{note.body}</div>
                    </article>
                }
            }).collect_view()}
        </div>
        <TruncationNote answer=answer />
    }
}
