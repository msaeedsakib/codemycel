use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_query_map};

use crate::app::api_client::{self, Answer, CommunityGraph, Line, NodeLine};
use crate::app::components::{
    Heuristic, Message, NodeLink, community_href, node_href, short_label,
};
use crate::app::{Refresh, ThemeChange};
use crate::canvas::render::{self, Edge, Scene, Shape, ShapeStyle, Theme, Transform};
use crate::canvas::{Link, Point, Viewport, force_layout, hit_test, spiral, spiral_radius};

const WIDTH: f64 = 1200.0;
const HEIGHT: f64 = 760.0;
const PADDING: f64 = 36.0;
const SEED: u64 = 0x00C0_DE5E_ED00;
const MEMBER_RADIUS: f64 = 4.5;
const MEMBER_SPACING: f64 = 13.0;
const MAX_MEMBERS: usize = 160;

/// What a shape on the map stands for.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Community(String),
    Hull,
    Member(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Map {
    scene: Scene,
    targets: Vec<Target>,
}

/// An expanded community: its id and the members to draw.
struct Expansion<'a> {
    id: &'a str,
    members: &'a [NodeLine],
}

/// Communities as circles sized by membership, joined by lines as thick as the relations
/// between them. Clicking a community expands it into its members.
#[component]
pub fn CommunityMap() -> impl IntoView {
    let refresh = expect_context::<Refresh>();
    let theme_change = expect_context::<ThemeChange>();
    let query = use_query_map();
    let navigate = use_navigate();
    let expanded = Memo::new(move |_| query.read().get("community").filter(|id| !id.is_empty()));

    let graph = LocalResource::new(move || {
        refresh.track();
        api_client::communities()
    });
    let detail = LocalResource::new(move || {
        refresh.track();
        let id = expanded.get();
        async move {
            match id {
                Some(id) => api_client::community(&id)
                    .await
                    .map(|answer| Some((id, answer))),
                None => Ok(None),
            }
        }
    });
    let current_detail = Memo::new(move |_| {
        let id = expanded.get()?;
        match detail.get()? {
            Ok(Some((loaded, answer))) if loaded == id => Some(answer),
            _ => None,
        }
    });
    let map = Memo::new(move |_| {
        let Some(Ok(graph)) = graph.get() else {
            return None;
        };
        let members = current_detail.get().map(|answer| members_of(&answer));
        let id = expanded.get();
        let expansion = match (&id, &members) {
            (Some(id), Some(members)) => Some(Expansion {
                id,
                members: members.as_slice(),
            }),
            _ => None,
        };
        Some(build_map(&graph, expansion))
    });

    let canvas = NodeRef::<leptos::html::Canvas>::new();
    let hovered = RwSignal::new(None::<usize>);
    let transform = StoredValue::new(Transform::default());
    let resized = RwSignal::new(0_u32);
    let resize = leptos::prelude::window_event_listener(leptos::ev::resize, move |_| {
        resized.update(|version| *version += 1)
    });
    on_cleanup(move || resize.remove());

    Effect::new(move |_| {
        theme_change.track();
        resized.track();
        let hovered = hovered.get();
        let Some(element) = canvas.get() else {
            return;
        };
        map.with(|map| {
            let scene = map.as_ref().map(|map| &map.scene);
            let empty = Scene::default();
            if let Ok(used) =
                render::draw(&element, scene.unwrap_or(&empty), hovered, &Theme::read())
            {
                transform.set_value(used);
            }
        });
    });

    let pointer_target = move |event: &web_sys::MouseEvent| -> Option<usize> {
        let at = transform.get_value().to_layout(Point::new(
            f64::from(event.offset_x()),
            f64::from(event.offset_y()),
        ));
        map.with_untracked(|map| {
            let map = map.as_ref()?;
            let hittable: Vec<usize> = (0..map.scene.shapes.len())
                .filter(|&index| map.targets[index] != Target::Hull)
                .collect();
            let positions: Vec<Point> = hittable.iter().map(|&i| map.scene.shapes[i].at).collect();
            let radii: Vec<f64> = hittable
                .iter()
                .map(|&i| map.scene.shapes[i].radius)
                .collect();
            hit_test(&positions, &radii, at).map(|found| hittable[found])
        })
    };
    let on_move = move |event: web_sys::PointerEvent| {
        let found = pointer_target(&event);
        if hovered.get_untracked() != found {
            hovered.set(found);
        }
    };
    let on_click = {
        let navigate = navigate.clone();
        move |event: web_sys::MouseEvent| {
            let found = pointer_target(&event);
            let target = map.with_untracked(|map| {
                map.as_ref()
                    .and_then(|map| found.and_then(|index| map.targets.get(index).cloned()))
            });
            hovered.set(None);
            let open = expanded.get_untracked();
            match target {
                Some(Target::Community(id)) if open.as_deref() != Some(id.as_str()) => {
                    navigate(&community_href(&id), NavigateOptions::default())
                }
                Some(Target::Member(path)) => {
                    navigate(&node_href(&path), NavigateOptions::default())
                }
                _ if open.is_some() => navigate("/", NavigateOptions::default()),
                _ => {}
            }
        }
    };

    view! {
        <div class="map-page">
            <div class="map-stage">
                <canvas
                    node_ref=canvas
                    class="map"
                    class:pointing=move || hovered.get().is_some()
                    on:pointermove=on_move
                    on:pointerleave=move |_| hovered.set(None)
                    on:click=on_click
                    aria-label="Community map"
                ></canvas>
                {move || match graph.get() {
                    None => Some(view! { <p class="map-overlay quiet">"Laying out communities…"</p> }.into_any()),
                    Some(Err(error)) => Some(view! { <div class="map-overlay"><Message text=error /></div> }.into_any()),
                    Some(Ok(graph)) if graph.communities.is_empty() => Some(view! {
                        <div class="map-overlay">
                            <Message text="No communities yet. They form once verified edges connect enough symbols." />
                        </div>
                    }.into_any()),
                    Some(Ok(_)) => None,
                }}
                <p class="legend quiet">
                    "Circle area: symbols in the community · Line weight: verified edges between communities"
                </p>
            </div>
            <aside class="map-side">
                {move || match expanded.get() {
                    Some(_) => match current_detail.get() {
                        Some(answer) => community_panel(answer).into_any(),
                        None => view! { <p class="quiet">"Loading…"</p> }.into_any(),
                    },
                    None => match graph.get() {
                        Some(Ok(graph)) => overview_panel(graph).into_any(),
                        _ => ().into_any(),
                    },
                }}
            </aside>
        </div>
    }
}

fn members_of(answer: &Answer) -> Vec<NodeLine> {
    answer
        .section("members")
        .iter()
        .filter_map(|line| match line {
            Line::Node(node) => Some(node.clone()),
            _ => None,
        })
        .take(MAX_MEMBERS)
        .collect()
}

fn community_radius(size: usize) -> f64 {
    (5.0 + 2.4 * (size as f64).sqrt()).min(44.0)
}

/// A stable palette slot for a community id.
fn color_of(id: &str) -> usize {
    id.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    }) as usize
}

fn build_map(graph: &CommunityGraph, expansion: Option<Expansion<'_>>) -> Map {
    let index_of = |id: &str| graph.communities.iter().position(|c| c.id == id);
    let expanded_index = expansion.as_ref().and_then(|e| index_of(e.id));
    let radii: Vec<f64> = graph
        .communities
        .iter()
        .enumerate()
        .map(|(index, community)| match (&expansion, expanded_index) {
            (Some(expansion), Some(open)) if open == index => {
                spiral_radius(expansion.members.len(), MEMBER_SPACING, 14.0)
            }
            _ => community_radius(community.size),
        })
        .collect();
    let links: Vec<Link> = graph
        .links
        .iter()
        .filter_map(|link| {
            Some(Link {
                source: index_of(&link.source)?,
                target: index_of(&link.target)?,
                weight: link.weight,
            })
        })
        .collect();
    let positions = force_layout(&radii, &links, Viewport::new(WIDTH, HEIGHT, PADDING), SEED);
    let heaviest = links
        .iter()
        .map(|link| link.weight)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let mut map = Map {
        scene: Scene {
            width: WIDTH,
            height: HEIGHT,
            shapes: Vec::new(),
            edges: links
                .iter()
                .map(|link| Edge {
                    source: link.source,
                    target: link.target,
                    width: 1.0 + 5.0 * (1.0 + link.weight as f64).ln() / (1.0 + heaviest).ln(),
                })
                .collect(),
        },
        targets: Vec::new(),
    };
    for (index, community) in graph.communities.iter().enumerate() {
        let open = expanded_index == Some(index);
        map.scene.shapes.push(Shape {
            at: positions[index],
            radius: radii[index],
            color: color_of(&community.id),
            label: short_label(&community.label),
            detail: format!(
                "{} {} · {}",
                community.size,
                if community.size == 1 {
                    "symbol"
                } else {
                    "symbols"
                },
                community.label
            ),
            style: if open {
                ShapeStyle::Hull
            } else {
                ShapeStyle::Community
            },
        });
        map.targets.push(if open {
            Target::Hull
        } else {
            Target::Community(community.id.clone())
        });
    }
    if let (Some(expansion), Some(open)) = (expansion, expanded_index) {
        let center = positions[open];
        let color = color_of(expansion.id);
        for (member, offset) in expansion
            .members
            .iter()
            .zip(spiral(expansion.members.len(), MEMBER_SPACING))
        {
            map.scene.shapes.push(Shape {
                at: Point::new(center.x + offset.x, center.y + offset.y),
                radius: MEMBER_RADIUS,
                color,
                label: member.short_name().to_owned(),
                detail: format!(
                    "{} · {} {}",
                    member.kind.replace('_', " "),
                    member.file.as_deref().unwrap_or_default(),
                    member.lines()
                ),
                style: ShapeStyle::Member,
            });
            map.targets.push(Target::Member(member.path.clone()));
        }
    }
    map
}

fn overview_panel(graph: CommunityGraph) -> impl IntoView {
    let mut communities = graph.communities.clone();
    communities.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.label.cmp(&b.label)));
    let symbols: usize = communities.iter().map(|c| c.size).sum();
    view! {
        <div class="side-head">
            <h1>"Communities"</h1>
            <Heuristic />
        </div>
        <p class="quiet">
            {format!(
                "{} communities · {} symbols · {} cross-links. Click a community to see its members.",
                communities.len(),
                symbols,
                graph.links.len()
            )}
        </p>
        <ul class="community-list">
            {communities.into_iter().map(|community| view! {
                <li>
                    <a href=community_href(&community.id) title=format!("community:{}", community.id)>
                        <span class="chip-dot" style=format!("--dot: var(--c{})", color_of(&community.id) % 8)></span>
                        <span class="label">{community.label}</span>
                        <span class="count">{community.size}</span>
                    </a>
                </li>
            }).collect_view()}
        </ul>
    }
}

fn community_panel(answer: Answer) -> impl IntoView {
    let mut heading = None;
    let mut fields = Vec::new();
    for line in answer.section("overview") {
        match line {
            Line::Community(community) => heading = Some(community.clone()),
            Line::Field { key, value } => fields.push((key.clone(), value.clone())),
            _ => {}
        }
    }
    let boundary = |name: &str| -> Vec<(Option<String>, String, String)> {
        answer
            .section(name)
            .iter()
            .filter_map(|line| match line {
                Line::Community(community) => Some((
                    Some(community.path.clone()),
                    community.label.clone(),
                    community.detail.clone().unwrap_or_default(),
                )),
                Line::Field { key, value } => Some((None, key.clone(), value.clone())),
                _ => None,
            })
            .collect()
    };
    let inbound = boundary("inbound");
    let outbound = boundary("outbound");
    let members = members_of(&answer);
    let hidden = answer.hidden("members")
        + answer
            .section("members")
            .len()
            .saturating_sub(members.len());
    let label = heading
        .as_ref()
        .map(|c| c.label.clone())
        .unwrap_or_default();
    let path = heading.map(|c| c.path).unwrap_or_default();
    view! {
        <div class="side-head">
            <a class="back" href="/" title="Collapse">"← All communities"</a>
        </div>
        <h1 class="side-title">{label}</h1>
        <div class="side-sub"><span class="mono quiet">{path}</span> <Heuristic /></div>
        <dl class="fields compact">
            {fields.into_iter().map(|(key, value)| view! { <dt>{key}</dt><dd>{value}</dd> }).collect_view()}
        </dl>
        <Boundary title="Inbound" lines=inbound />
        <Boundary title="Outbound" lines=outbound />
        <section class="side-section">
            <h2>"Members " <span class="count">{members.len() + hidden}</span></h2>
            <ul class="plain-list members">
                {members.into_iter().map(|node| view! { <li><NodeLink node=node /></li> }).collect_view()}
            </ul>
            {(hidden > 0).then(|| view! { <p class="quiet">{format!("{hidden} more not shown")}</p> })}
        </section>
    }
}

#[component]
fn Boundary(
    #[prop(into)] title: String,
    lines: Vec<(Option<String>, String, String)>,
) -> impl IntoView {
    (!lines.is_empty()).then(|| {
        view! {
            <section class="side-section">
                <h2>{title}</h2>
                <ul class="plain-list">
                    {lines.into_iter().map(|(path, label, detail)| view! {
                        <li class="boundary">
                            {match path {
                                Some(path) => view! { <a href=community_href(&path)>{label}</a> }.into_any(),
                                None => view! { <span class="quiet">{label}</span> }.into_any(),
                            }}
                            <span class="count">{detail}</span>
                        </li>
                    }).collect_view()}
                </ul>
            </section>
        }
    })
}
