// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Material motion system (milestone 785): fade through between the tabs, a shared
//! axis between steps and onto a page, a fade scale, items animating in and out, a
//! staggered entrance and a card that grows into a page.
//!
//! `cargo run -p frus-demo --example motion` on the desktop.
//!
//! - `FRUS_MOTION_SPEED=0.1` slows every animation down tenfold, to look at it.
//! - `FRUS_MOTION_AUTO=1` plays every change by itself, every two seconds.
//! - `FRUS_MOTION_TAB=1` (or 2) starts on another tab.

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn main() -> frus_shell::anyhow::Result<()> {
    use frus_shell::FrusApp;
    use frus_widgets::{
        component, text, AnimatedPresence, AnimatedSwitcher, BottomNavigationBar,
        BottomNavigationBarItem, Button, Color, Container, Flex, GoRoute, GoRouter, Icons,
        MotionPattern, MotionPreset, OpenContainer, PageTransitionsBuilder, PresenceList, Scaffold,
        SharedAxis, ShellBranch, SingleChildScrollView, Staggered, Transition, Widget,
    };
    use std::time::Duration;

    if let Some(speed) = std::env::var("FRUS_MOTION_SPEED")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
    {
        frus_widgets::set_animation_speed(speed);
    }
    let auto = std::env::var("FRUS_MOTION_AUTO").is_ok_and(|v| v == "1");
    let tick = Duration::from_millis(2000);

    fn heading(label: &str) -> Container {
        Container::new()
            .padding_each(16.0, 16.0, 8.0, 16.0)
            .child(text(label.to_string()).size(18.0))
    }

    fn panel(colour: Color, label: String) -> Container {
        Container::new()
            .width(280.0)
            .height(120.0)
            .radius(16.0)
            .color(colour)
            .padding(16.0)
            .child(text(label).size(20.0).color(Color::WHITE))
    }

    let patterns = move |auto: bool| {
        component(move |cx| {
            let step = cx.use_state(|| 0_u32);
            let forward = cx.use_state(|| true);
            let shown = cx.use_state(|| true);
            if auto {
                let (s, f, v) = (step.clone(), forward.clone(), shown.clone());
                cx.use_interval(tick, move || {
                    s.update(|n| *n = (*n + 1) % 3);
                    f.set(true);
                    v.update(|b| *b = !*b);
                });
            }
            let colours = [
                Color::rgb(0.20, 0.35, 0.65),
                Color::rgb(0.55, 0.25, 0.50),
                Color::rgb(0.15, 0.50, 0.40),
            ];
            let n = step.get();
            let next = {
                let (s, f) = (step.clone(), forward.clone());
                move || {
                    s.update(|n| *n = (*n + 1) % 3);
                    f.set(true);
                }
            };
            let back = {
                let (s, f) = (step.clone(), forward.clone());
                move || {
                    s.update(|n| *n = (*n + 2) % 3);
                    f.set(false);
                }
            };
            let toggle = {
                let v = shown.clone();
                move || v.update(|b| *b = !*b)
            };
            let open_page = {
                let router = cx.router();
                move || router.push("/details")
            };
            Box::new(
                SingleChildScrollView::new().child(
                    Flex::column()
                        .child(heading("Shared axis X between steps"))
                        .child(
                            Container::new().padding_each(0.0, 16.0, 0.0, 16.0).child(
                                AnimatedSwitcher::patterned(
                                    MotionPattern::SharedAxis(SharedAxis::X),
                                    n,
                                    move |n: &u32| {
                                        panel(colours[*n as usize], format!("Step {}", n + 1))
                                    },
                                )
                                .reverse(!forward.get()),
                            ),
                        )
                        .child(
                            Flex::row()
                                .gap(8.0)
                                .padding(16.0)
                                .child(Button::new("Back").on_press(back))
                                .child(Button::new("Next").on_press(next)),
                        )
                        .child(heading("Fade through"))
                        .child(Container::new().padding_each(0.0, 16.0, 0.0, 16.0).child(
                            AnimatedSwitcher::patterned(
                                MotionPattern::FadeThrough,
                                n,
                                move |n: &u32| {
                                    panel(
                                        colours[(*n as usize + 1) % 3],
                                        format!("Album {}", n + 1),
                                    )
                                },
                            ),
                        ))
                        .child(heading("Fade scale, in and out"))
                        .child(
                            Flex::row()
                                .gap(16.0)
                                .padding(16.0)
                                .child(Button::new("Toggle").on_press(toggle))
                                .child(
                                    AnimatedPresence::new(
                                        shown.get(),
                                        panel(Color::rgb(0.65, 0.35, 0.15), "A menu".to_string())
                                            .width(160.0),
                                    )
                                    .enter(
                                        Transition::fade(0.15, frus_widgets::Curve::Linear)
                                            .scale(0.8),
                                    )
                                    .exit(Transition::preset(MotionPreset::StandardExit)),
                                ),
                        )
                        .child(heading("Shared axis Z onto a page"))
                        .child(
                            Container::new()
                                .padding(16.0)
                                .child(Button::new("Open the details").on_press(open_page)),
                        ),
                ),
            ) as Box<dyn Widget>
        })
    };

    let lists = move |auto: bool| {
        component(move |cx| {
            let items = cx.use_state(|| vec![1_u32, 2, 3]);
            let next = cx.use_state(|| 4_u32);
            let add = {
                let (items, next) = (items.clone(), next.clone());
                move || {
                    let n = next.get();
                    next.set(n + 1);
                    items.update(|v| v.insert(1.min(v.len()), n));
                }
            };
            let remove = {
                let items = items.clone();
                move || {
                    items.update(|v| {
                        if !v.is_empty() {
                            v.remove(v.len() / 2);
                        }
                    })
                }
            };
            // Each round of the automatic mode builds the staggered column afresh, so its
            // entrance plays again.
            let round = cx.use_state(|| 0_u32);
            if auto {
                let (add, remove) = (add.clone(), remove.clone());
                let flip = cx.use_state(|| false);
                let again = round.clone();
                cx.use_interval(tick, move || {
                    if flip.get() {
                        remove();
                    } else {
                        add();
                        again.update(|r| *r += 1);
                    }
                    flip.update(|f| *f = !*f);
                });
            }
            let row = |n: &u32| {
                Container::new().padding_each(4.0, 16.0, 4.0, 16.0).child(
                    Container::new()
                        .height(48.0)
                        .radius(12.0)
                        .color(Color::rgb(0.22, 0.24, 0.32))
                        .padding(14.0)
                        .child(text(format!("Item {n}"))),
                )
            };
            Box::new(
                SingleChildScrollView::new().child(
                    Flex::column()
                        .child(heading("Items in and out"))
                        .child(
                            Flex::row()
                                .gap(8.0)
                                .padding(16.0)
                                .child(Button::new("Add").on_press(add))
                                .child(Button::new("Remove").on_press(remove)),
                        )
                        .child(PresenceList::new(items.get(), |n| *n, row).stagger(0.05))
                        .child(heading("Staggered entrance"))
                        .child(frus_widgets::Keyed::new(
                            round.get(),
                            Staggered::column()
                                .gap(8.0)
                                .interval(0.08)
                                .child(
                                    panel(Color::rgb(0.20, 0.35, 0.65), "One".into()).height(56.0),
                                )
                                .child(
                                    panel(Color::rgb(0.55, 0.25, 0.50), "Two".into()).height(56.0),
                                )
                                .child(
                                    panel(Color::rgb(0.15, 0.50, 0.40), "Three".into())
                                        .height(56.0),
                                ),
                        )),
                ),
            ) as Box<dyn Widget>
        })
    };

    let cards = move |auto: bool| {
        component(move |cx| {
            let open = cx.use_state(|| false);
            if auto {
                let o = open.clone();
                cx.use_interval(tick, move || o.update(|b| *b = !*b));
            }
            let card = |label: &'static str| {
                OpenContainer::builder(
                    move |_| {
                        Container::new()
                            .width(150.0)
                            .height(150.0)
                            .padding(12.0)
                            .child(text(label))
                    },
                    move |close| {
                        Scaffold::new().body(
                            Flex::column()
                                .padding(24.0)
                                .gap(16.0)
                                .child(text(format!("{label}, open")).size(28.0))
                                .child(text("The card grew into this page."))
                                .child(Button::new("Close").on_press(close)),
                        )
                    },
                )
            };
            let (o1, o2) = (open.clone(), open.clone());
            let controlled = OpenContainer::new(
                open.get(),
                Container::new()
                    .width(316.0)
                    .height(90.0)
                    .padding(12.0)
                    .child(text("Tap me: a card that becomes a page")),
                Scaffold::new().body(
                    Flex::column()
                        .padding(24.0)
                        .gap(16.0)
                        .child(text("Details").size(28.0))
                        .child(Button::new("Close").on_press(move || o2.set(false))),
                ),
            )
            .on_open(move || o1.set(true));
            Box::new(
                Flex::column()
                    .child(heading("Container transform"))
                    .child(
                        Flex::row()
                            .gap(16.0)
                            .padding(16.0)
                            .child(card("Lisbon"))
                            .child(card("Porto")),
                    )
                    .child(
                        Container::new()
                            .padding_each(0.0, 16.0, 0.0, 16.0)
                            .child(controlled),
                    ),
            ) as Box<dyn Widget>
        })
    };

    let start_tab: usize = std::env::var("FRUS_MOTION_TAB")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let starts = ["/patterns", "/lists", "/cards"];
    let router = GoRouter::new(vec![
        GoRoute::shell(
            |_, shell, body| {
                let tabs = shell.clone();
                Scaffold::new().body(body).bottom_navigation_bar(
                    BottomNavigationBar::new(shell.current_index(), move |i| tabs.go_branch(i))
                        .items(vec![
                            BottomNavigationBarItem::new(Icons::HOME, "Patterns"),
                            BottomNavigationBarItem::new(Icons::LIST, "Lists"),
                            BottomNavigationBarItem::new(Icons::SETTINGS, "Cards"),
                        ]),
                )
            },
            vec![
                ShellBranch::new(vec![GoRoute::new("/patterns", move |_, _| patterns(auto))]),
                ShellBranch::new(vec![GoRoute::new("/lists", move |_, _| lists(auto))]),
                ShellBranch::new(vec![GoRoute::new("/cards", move |_, _| cards(auto))]),
            ],
        )
        // The destinations of a navigation bar fade through.
        .branch_transition(PageTransitionsBuilder::FadeThrough),
        GoRoute::new("/details", |cx, _| {
            let router = cx.router();
            Scaffold::new().body(
                Flex::column()
                    .padding(24.0)
                    .gap(16.0)
                    .child(text("A page one level down").size(28.0))
                    .child(text("It came in along the Z axis."))
                    .child(Button::new("Back").on_press(move || {
                        router.pop();
                    })),
            )
        })
        // A parent and its child: the shared Z axis.
        .transition(PageTransitionsBuilder::SharedAxis(SharedAxis::Z)),
    ])
    .initial_location(starts[start_tab.min(2)]);

    let app = FrusApp::router(router).title("frus motion");
    frus_shell::run(app)
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn main() {}
