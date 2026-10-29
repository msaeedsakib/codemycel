use leptos::prelude::*;

use crate::app::Refresh;
use crate::app::api_client::{self, Answer};
use crate::app::components::{TruncationNote, loaded, when};

/// The index, its roots and their verification coverage, counts and diagnostics.
#[component]
pub fn Status() -> impl IntoView {
    let refresh = expect_context::<Refresh>();
    let answer = LocalResource::new(move || {
        refresh.track();
        api_client::status()
    });
    view! {
        <div class="page narrow">
            <div class="page-head"><h1>"Status and coverage"</h1></div>
            {loaded(answer, status_view)}
        </div>
    }
}

fn status_view(answer: Answer) -> impl IntoView {
    let index: Vec<(String, String)> = answer
        .fields("index")
        .into_iter()
        .map(|(key, value)| {
            let value = if key == "indexed" {
                when(&value)
            } else {
                value
            };
            (key, value)
        })
        .collect();
    let coverage = answer.freshness.coverage.clone();
    view! {
        <section class="panel">
            <h2>"Coverage"</h2>
            <div class="coverage">
                {coverage.into_iter().map(|language| view! {
                    <div class="coverage-row">
                        <span class="language">{language.language}</span>
                        {language.states.into_iter().map(|state| {
                            let verified = state.is_verified();
                            view! { <span class="tag" class:ok=verified class:warn=!verified>{state.text()}</span> }
                        }).collect_view()}
                    </div>
                }).collect_view()}
            </div>
        </section>
        <Fields title="Index" fields=index />
        <Fields title="Roots" fields=answer.fields("roots") />
        <Fields title="Counts" fields=answer.fields("counts") />
        <Fields title="Diagnostics" fields=answer.fields("diagnostics") />
        <Fields title="Partially enriched files" fields=answer.fields("partially_enriched") />
        <Fields title="Skipped files" fields=answer.fields("skipped") />
        <TruncationNote answer=answer />
    }
}

#[component]
fn Fields(#[prop(into)] title: String, fields: Vec<(String, String)>) -> impl IntoView {
    (!fields.is_empty()).then(|| {
        view! {
            <section class="panel">
                <h2>{title}</h2>
                <dl class="fields">
                    {fields.into_iter().map(|(key, value)| view! {
                        <dt>{key}</dt>
                        <dd>{value}</dd>
                    }).collect_view()}
                </dl>
            </section>
        }
    })
}
