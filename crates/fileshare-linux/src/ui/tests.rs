use std::cell::Cell;
use std::rc::Rc;

use anyhow::Context;
use crowntest::{Role, TestApp, role, text};
use crownui::kit::{Theme, provide_theme};
use crownui::prelude::component;
use ffsp_protocol::medium::Medium;
use ffsp_protocol::wire::LaneId;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::ui::root::shell;
use crate::ui::state::{RigCommand, RigEvent, Store};

const WIDTH: f32 = 980.0;
const HEIGHT: f32 = 700.0;

struct Window {
    app: TestApp,
    store: Store,
    commands: UnboundedReceiver<RigCommand>,
}

impl Window {
    fn open() -> anyhow::Result<Self> {
        let (sender, commands) = unbounded_channel();
        let slot = Rc::new(Cell::new(None));
        let published = Rc::clone(&slot);
        let mut app = TestApp::with_size(
            component(move |cx| {
                provide_theme(cx, Theme::light());
                let store = Store::new(cx);
                store.attach(cx, sender.clone());
                published.set(Some(store));
                shell(store)
            }),
            WIDTH,
            HEIGHT,
        );
        app.settle();
        let store = slot.get().context("the root builds synchronously")?;
        Ok(Self {
            app,
            store,
            commands,
        })
    }

    fn apply(&mut self, event: RigEvent) {
        let store = self.store;
        store.apply(self.app.ui().cx(), event);
        self.app.settle();
    }

    fn open_benchmark(&mut self) {
        self.app.click(text("Benchmark"));
        self.app.settle();
    }

    fn click_receive(&mut self) {
        self.app.click(role(Role::Button).name("Receive"));
        self.app.settle();
    }

    fn receive_is_disabled(&mut self) -> bool {
        self.app
            .find(role(Role::Button).name("Receive"))
            .states
            .disabled
    }

    fn sent(&mut self) -> Vec<RigCommand> {
        std::iter::from_fn(|| self.commands.try_recv().ok()).collect()
    }
}

#[test]
fn benchmark_explains_that_nothing_was_measured() -> anyhow::Result<()> {
    let mut window = Window::open()?;
    window.open_benchmark();
    assert!(window.app.exists(text("Nothing measured yet")));
    Ok(())
}

#[test]
fn benchmark_shows_each_lane_that_reports() -> anyhow::Result<()> {
    let mut window = Window::open()?;
    window.apply(RigEvent::Lane {
        lane: LaneId::new(1),
        medium: Medium::WifiInfra,
        kbps: 80_000,
        bytes: 4_096,
    });
    window.open_benchmark();
    assert!(!window.app.exists(text("Nothing measured yet")));
    assert!(window.app.exists(text("Wi-Fi")));
    assert!(window.app.exists(text("100.0%")));
    Ok(())
}

#[test]
fn receive_locks_the_actions_until_the_rig_finishes() -> anyhow::Result<()> {
    let mut window = Window::open()?;
    window.click_receive();
    assert!(matches!(window.sent().as_slice(), [RigCommand::Receive]));
    assert!(window.receive_is_disabled());

    window.apply(RigEvent::Failed("no phone in range".to_owned()));
    assert!(window.app.exists(text("no phone in range")));
    assert!(!window.receive_is_disabled());
    window.click_receive();
    assert!(matches!(window.sent().as_slice(), [RigCommand::Receive]));
    Ok(())
}
