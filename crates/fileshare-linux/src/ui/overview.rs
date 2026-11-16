use crownui::kit::{
    ButtonOptions, ButtonVariant, StatusOptions, Tone, button, divider, icons, progress, status,
};
use crownui::prelude::{
    Cx, Listeners, Semantic, Signal, Styled, View, bind, for_each, hstack, show, spacer, vstack,
};
use ffsp_protocol::medium::Medium;

use crate::ui::state::{RigCommand, Store, fraction};
use crate::ui::text::{body, heading};
use crate::utils::nothing::nothing;

const MEDIUM_COLUMN_WIDTH: f32 = 160.0;

pub(crate) fn overview(store: Store) -> impl View {
    let mediums = for_each(
        store.readiness,
        |readiness: &(Medium, bool)| *readiness,
        medium_row,
    )
    .flex_col()
    .gap(4.0);

    let chosen_line = bind(move |runtime| {
        store.chosen.with(runtime, |chosen| {
            chosen.as_ref().map_or_else(
                || "No file chosen".to_owned(),
                |path| path.display().to_string(),
            )
        })
    });

    let transfer_progress = show(
        bind(move |runtime| store.progress.with(runtime, Option::is_some)),
        move || progress(bind(move |runtime| fraction(store.progress.get(runtime)))),
        nothing,
    );

    vstack((
        heading("Mediums"),
        mediums,
        divider(),
        heading("Transfer"),
        body(chosen_line),
        actions(store),
        transfer_progress,
        divider(),
        body(store.status),
    ))
    .gap(12.0)
}

fn medium_row(cx: &mut Cx, readiness: Signal<(Medium, bool)>) -> impl View + use<> {
    let (medium, ready) = readiness.with_untracked(cx, |readiness| *readiness);
    let (tone, description) = if ready {
        (Tone::Success, "ready")
    } else {
        (Tone::Danger, "unavailable — the run will use the others")
    };
    hstack((
        hstack((status(medium.to_string()).tone(tone),)).w(MEDIUM_COLUMN_WIDTH),
        body(description),
    ))
    .gap(8.0)
    .items_center()
}

fn actions(store: Store) -> impl View {
    let busy = store.busy;
    let nothing_to_send =
        bind(move |runtime| busy.get(runtime) || store.chosen.with(runtime, Option::is_none));
    hstack((
        button("Receive")
            .variant(ButtonVariant::Primary)
            .leading_icon(icons::DOWNLOAD)
            .disabled(busy)
            .on_click(move |cx, _| store.command(cx, RigCommand::Receive)),
        button("Choose a file…")
            .leading_icon(icons::FOLDER_OPEN)
            .disabled(busy)
            .on_click(move |cx, _| choose_file(cx, store)),
        button("Send")
            .leading_icon(icons::UPLOAD)
            .disabled(nothing_to_send)
            .on_click(move |cx, _| {
                let chosen = store.chosen.with_untracked(cx, Clone::clone);
                if let Some(path) = chosen {
                    store.command(cx, RigCommand::Send(path));
                }
            }),
        spacer(),
        button("Self test")
            .variant(ButtonVariant::Ghost)
            .disabled(busy)
            .on_click(move |cx, _| store.command(cx, RigCommand::SelfTest)),
    ))
    .gap(8.0)
    .w_full()
}

fn choose_file(cx: &mut Cx, store: Store) {
    cx.spawn(rfd::AsyncFileDialog::new().pick_file(), move |cx, file| {
        if let Some(file) = file {
            store.chosen.set(cx, Some(file.path().to_path_buf()));
        }
    });
}
