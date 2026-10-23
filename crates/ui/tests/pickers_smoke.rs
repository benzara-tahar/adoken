//! Pickers through real gpui key dispatch (spec 0015 FR-11/13/14/21; 0004
//! FR-18): typing filters, `↑ ↓ ← → ↩ Esc` reach the focused picker,
//! choices are emitted, and the keyboard returns to the host.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("pickers_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use adoken_theme::{ThemeRegistry, size};
    use adoken_ui::Theme;
    use adoken_ui::picker::{
        DayRange, Directory, LinkType, LinkTypeEvent, LinkTypeMenu, PeopleChoice, PeopleEvent,
        PeopleMode, PeoplePicker, Person, PickerSlot, Placement, StateCategory, StateMenu,
        StateMenuEvent, StateOption, TreeEvent, TreeKind, TreeNode, TreePicker, nodes_from_paths,
    };
    use anyhow::{Context as _, ensure};
    use gpui::{
        AppContext as _, Context, FocusHandle, HeadlessAppContext, Keystroke, Render, Subscription,
        Window, WindowHandle, div, point, prelude::*, px, size as gsize,
    };

    #[derive(Default)]
    struct Host {
        focus: Option<FocusHandle>,
        state: PickerSlot<StateMenu>,
        link: PickerSlot<LinkTypeMenu>,
        tree: PickerSlot<TreePicker>,
        people: PickerSlot<PeoplePicker>,
        chosen: Vec<String>,
        subscriptions: Vec<Subscription>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(self.focus.as_ref().unwrap())
                .children(self.state.element())
                .children(self.link.element())
                .children(self.tree.element())
                .children(self.people.element())
        }
    }

    fn placement() -> Placement {
        Placement::at(
            point(px(20.0), px(20.0)),
            gpui::Anchor::TopLeft,
            size::PICKER_POPOVER,
        )
        .keyboard()
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx);
            cx.bind_keys(adoken_ui::text_input::bindings());
            cx.bind_keys(adoken_ui::picker::bindings());
        });
        let window = cx.open_window(gsize(px(900.0), px(700.0)), |window, cx| {
            cx.new(|cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                Host {
                    focus: Some(focus),
                    ..Default::default()
                }
            })
        })?;
        state_menu(&mut cx, window)?;
        link_menu(&mut cx, window)?;
        tree_picker(&mut cx, window)?;
        people_picker(&mut cx, window)?;
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        Ok(())
    }

    fn press(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Host>,
        keys: &str,
    ) -> anyhow::Result<()> {
        for key in keys.split(' ') {
            let keystroke = Keystroke::parse(key).context("keystroke")?;
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.dispatch_keystroke(keystroke, cx);
            })?;
            cx.run_until_parked();
        }
        Ok(())
    }

    fn host_focused(cx: &mut HeadlessAppContext, window: WindowHandle<Host>) -> bool {
        window
            .update(cx, |host, window, _| {
                host.focus.as_ref().unwrap().is_focused(window)
            })
            .unwrap_or(false)
    }

    fn take_chosen(cx: &mut HeadlessAppContext, window: WindowHandle<Host>) -> Vec<String> {
        window
            .update(cx, |host, _, _| std::mem::take(&mut host.chosen))
            .unwrap_or_default()
    }

    fn state_menu(cx: &mut HeadlessAppContext, window: WindowHandle<Host>) -> anyhow::Result<()> {
        let open = |cx: &mut HeadlessAppContext| {
            window.update(cx, |host, window, cx| {
                use StateCategory::*;
                let states = vec![
                    StateOption::new("New", Proposed),
                    StateOption::new("Doing", InProgress),
                    StateOption::new("Code review", InProgress),
                    StateOption::new("Resolved", Resolved),
                    StateOption::new("Closed", Completed),
                ];
                let menu = cx.new(|cx| {
                    StateMenu::new(
                        "Change state",
                        "Doing",
                        Some(states),
                        placement(),
                        window,
                        cx,
                    )
                });
                host.subscriptions.push(
                    cx.subscribe(&menu, |host, _, StateMenuEvent::Chosen(state), _| {
                        host.chosen.push(state.to_string())
                    }),
                );
                host.state.open(menu, |h| &mut h.state, cx);
            })
        };
        open(cx)?;
        cx.run_until_parked();
        ensure!(!host_focused(cx, window), "the menu takes the keyboard");
        // Spec §3.2: `r e s ↩`.
        press(cx, window, "r e s enter")?;
        ensure!(
            take_chosen(cx, window) == ["Resolved"],
            "typing filters, enter sends"
        );
        ensure!(host_focused(cx, window), "focus returns to the host");
        ensure!(
            window.update(cx, |h, _, _| !h.state.is_open())?,
            "slot empties"
        );

        // Unfiltered: the selection starts after the current state; ↓ moves.
        open(cx)?;
        press(cx, window, "down enter")?;
        ensure!(
            take_chosen(cx, window) == ["Resolved"],
            "Code review → ↓ → Resolved"
        );

        // Esc clears the filter first, then closes without a choice.
        open(cx)?;
        press(cx, window, "z z escape")?;
        ensure!(
            window.update(cx, |h, _, _| h.state.is_open())?,
            "first Esc clears the filter"
        );
        press(cx, window, "escape")?;
        ensure!(
            window.update(cx, |h, _, _| !h.state.is_open())? && host_focused(cx, window),
            "second Esc closes and returns focus"
        );
        ensure!(take_chosen(cx, window).is_empty(), "Esc sends nothing");
        println!("pickers_smoke: state menu filters, chooses, Esc clears then closes");
        Ok(())
    }

    fn link_menu(cx: &mut HeadlessAppContext, window: WindowHandle<Host>) -> anyhow::Result<()> {
        let open = |cx: &mut HeadlessAppContext| {
            window.update(cx, |host, window, cx| {
                let menu = cx.new(|cx| LinkTypeMenu::new(true, placement(), window, cx));
                host.subscriptions.push(
                    cx.subscribe(&menu, |host, _, LinkTypeEvent::Chosen(ty), _| {
                        host.chosen.push(ty.rel().into())
                    }),
                );
                host.link.open(menu, |h| &mut h.link, cx);
            })
        };
        open(cx)?;
        press(cx, window, "3")?;
        ensure!(
            take_chosen(cx, window).is_empty(),
            "digits pick nothing (spec 0025 FR-3)"
        );
        press(cx, window, "down down enter")?;
        ensure!(
            take_chosen(cx, window) == [LinkType::Related.rel()],
            "↓ ↓ ↩ picks Related"
        );
        ensure!(host_focused(cx, window), "focus returns after a pick");
        open(cx)?;
        press(cx, window, "up enter")?;
        ensure!(
            take_chosen(cx, window) == [LinkType::DuplicateOf.rel()],
            "↑ wraps to Duplicate of"
        );
        println!("pickers_smoke: link-type menu arrows, no digits");
        Ok(())
    }

    fn tree_picker(cx: &mut HeadlessAppContext, window: WindowHandle<Host>) -> anyhow::Result<()> {
        let open = |cx: &mut HeadlessAppContext, kind: TreeKind, nodes: Vec<TreeNode>, current| {
            window.update(cx, |host, window, cx| {
                let picker = cx.new(|cx| {
                    TreePicker::new(
                        kind,
                        "Change area",
                        Some(nodes),
                        Some(current),
                        placement(),
                        window,
                        cx,
                    )
                });
                host.subscriptions.push(
                    cx.subscribe(&picker, |host, _, TreeEvent::Chosen(path), _| {
                        host.chosen.push(path.to_string())
                    }),
                );
                host.tree.open(picker, |h| &mut h.tree, cx);
            })
        };
        let areas = || {
            nodes_from_paths(&[
                "Storefront\\Infra\\Payments",
                "Storefront\\Infra\\Search",
                "Storefront\\Web\\Checkout",
            ])
        };
        // Opens on the current path; ← goes to the parent, ← collapses it,
        // → expands it again, → steps into its first child.
        open(
            cx,
            TreeKind::Area,
            areas(),
            "Storefront\\Infra\\Search".into(),
        )?;
        press(cx, window, "left enter")?;
        ensure!(
            take_chosen(cx, window) == ["Storefront\\Infra"],
            "← from a leaf selects its parent"
        );
        ensure!(host_focused(cx, window), "focus returns after choosing");
        open(
            cx,
            TreeKind::Area,
            areas(),
            "Storefront\\Infra\\Search".into(),
        )?;
        press(cx, window, "left left down enter")?;
        ensure!(
            take_chosen(cx, window) == ["Storefront\\Web"],
            "← on an open parent collapses it"
        );
        open(cx, TreeKind::Area, areas(), "Storefront\\Web".into())?;
        press(cx, window, "right right enter")?;
        ensure!(
            take_chosen(cx, window) == ["Storefront\\Web\\Checkout"],
            "→ expands, then steps into the first child"
        );
        // Typing filters by any segment; the first match is selected.
        open(cx, TreeKind::Area, areas(), "Storefront".into())?;
        press(cx, window, "c h e enter")?;
        ensure!(
            take_chosen(cx, window) == ["Storefront\\Web\\Checkout"],
            "filtered: first match chosen"
        );

        // Iteration: the current sprint is selected in the Current group.
        let mut sprints = vec![
            TreeNode::new("Storefront", None),
            TreeNode::new("2026", Some(0)),
        ];
        for (name, start) in [("Sprint 43", 78), ("Sprint 44", 92), ("Sprint 45", 106)] {
            sprints.push(TreeNode::new(name, Some(1)).dates(DayRange {
                start,
                finish: start + 13,
            }));
        }
        open(
            cx,
            TreeKind::Iteration { today: 100 },
            sprints,
            "Storefront\\2026\\Sprint 43".into(),
        )?;
        press(cx, window, "escape")?;
        ensure!(
            take_chosen(cx, window).is_empty() && host_focused(cx, window),
            "Esc closes without a choice"
        );
        println!("pickers_smoke: tree picker arrows, filter, Esc");
        Ok(())
    }

    fn people_picker(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Host>,
    ) -> anyhow::Result<()> {
        let me = Person::new("me", "Ada Lindqvist", "ada@contoso.com");
        let people = vec![
            Person::new("1", "Dana Kim", "dana@contoso.com"),
            Person::new("2", "Kwame Mensah", "kwame@contoso.com"),
        ];
        let open = |cx: &mut HeadlessAppContext| {
            window.update(cx, |host, window, cx| {
                let picker = cx.new(|cx| {
                    let mut picker = PeoplePicker::new(
                        PeopleMode::Assign { me: me.clone() },
                        placement(),
                        window,
                        cx,
                    );
                    picker.set_people(people.clone(), cx);
                    picker
                });
                host.subscriptions.push(cx.subscribe(
                    &picker,
                    |host, _, event: &PeopleEvent, _| {
                        host.chosen.push(match event {
                            PeopleEvent::QueryChanged(q) => format!("query:{q}"),
                            PeopleEvent::Chosen(PeopleChoice::AssignToMe) => "me".into(),
                            PeopleEvent::Chosen(PeopleChoice::Unassigned) => "none".into(),
                            PeopleEvent::Chosen(PeopleChoice::Person(p)) => p.name.to_string(),
                            PeopleEvent::RetryDirectory => "retry".into(),
                        })
                    },
                ));
                host.people.open(picker, |h| &mut h.people, cx);
            })
        };
        open(cx)?;
        press(cx, window, "enter")?;
        ensure!(
            take_chosen(cx, window) == ["me"],
            "first row is Assign to me"
        );
        open(cx)?;
        press(cx, window, "k w a enter")?;
        ensure!(
            take_chosen(cx, window) == ["query:k", "query:kw", "query:kwa", "Kwame Mensah"],
            "typing reports the query and ↩ picks the first person match"
        );
        // A directory result arriving after the user moved keeps the
        // selection; the failed group's row retries on ↩.
        open(cx)?;
        press(cx, window, "z z z")?;
        window.update(cx, |host, _, cx| {
            host.people
                .picker()
                .unwrap()
                .update(cx, |picker, cx| picker.set_directory(Directory::Failed, cx))
        })?;
        press(cx, window, "up enter")?;
        let events = take_chosen(cx, window);
        ensure!(
            events.last().map(String::as_str) == Some("retry"),
            "↩ on the failed directory row asks for a retry: {events:?}"
        );
        ensure!(
            window.update(cx, |h, _, _| h.people.is_open())?,
            "retry keeps the picker open"
        );
        press(cx, window, "escape")?;
        ensure!(host_focused(cx, window), "Esc returns focus");
        println!("pickers_smoke: people picker assign-to-me, typed match, directory retry");
        Ok(())
    }
}
