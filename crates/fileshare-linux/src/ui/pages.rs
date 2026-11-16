use crownui::prelude::{Styled, View, keyed};

use crate::ui::benchmark::benchmark;
use crate::ui::overview::overview;
use crate::ui::state::{Page, Store};

pub(crate) fn page(store: Store) -> impl View + Styled {
    keyed(
        move |runtime| store.page.get(runtime),
        move |_, page| match page {
            Page::Overview => overview(store).boxed(),
            Page::Benchmark => benchmark(store).boxed(),
        },
    )
}
