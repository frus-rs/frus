// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! One module per screen, and the routes that say where each one lives.
//!
//! A screen is a widget — a `StatelessWidget` when it only shows what it is given, a
//! `StatefulWidget` when it keeps something — and a route is the address it answers to. The
//! router builds the widget for the address it was told, keeps the ones under it while
//! another is on top, and moves between them with the slide and the back gesture. Which route
//! is open, how deep the stack is and what each page was told when it was built are the
//! router's; nothing here keeps them.

mod board;
mod browser;
mod camera;
mod charts;
mod conversation;
mod cupertino;
mod data;
mod display;
mod grid;
mod inputs;
mod journal;
mod licenses;
mod menu;
mod native;
mod overlays;
mod player;
mod questionnaire;
mod settings;
mod sheet;
mod shop;
mod task;
mod todo;
mod tour;
mod video;
mod wizard;

pub(crate) use board::*;
pub(crate) use browser::*;
pub(crate) use camera::*;
pub(crate) use charts::*;
pub(crate) use conversation::*;
pub(crate) use cupertino::*;
pub(crate) use data::*;
pub(crate) use display::*;
pub(crate) use grid::*;
pub(crate) use inputs::*;
pub(crate) use journal::*;
pub(crate) use licenses::*;
pub(crate) use menu::window;
pub(crate) use native::*;
pub(crate) use overlays::*;
pub(crate) use player::*;
pub(crate) use questionnaire::*;
pub(crate) use settings::*;
pub(crate) use sheet::*;
pub(crate) use shop::*;
pub(crate) use task::*;
pub(crate) use todo::*;
pub(crate) use tour::*;
pub(crate) use video::*;
pub(crate) use wizard::*;

use crate::prelude::*;

/// **Where each screen lives**: the demo's routes, typed. A location is made from a variant
/// (`Route::Task { id: 3 }.location()` is `/task/3`), so a misspelt screen does not compile;
/// and a location from outside (a deep link, the browser's address bar, `frusdemo://task/3`)
/// still reads as one, through the router or [`Routes::from_location`].
#[derive(Routes, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// The task list.
    #[route("/")]
    Home,
    #[route("/settings")]
    Settings,
    #[route("/wizard")]
    Wizard,
    #[route("/grid")]
    Grid,
    #[route("/journal")]
    Journal,
    #[route("/charts")]
    Charts,
    #[route("/data")]
    Data,
    #[route("/board")]
    Board,
    #[route("/tour")]
    Tour,
    #[route("/sheet")]
    Sheet,
    #[route("/licenses")]
    Licenses,
    #[route("/video")]
    Video,
    #[route("/camera")]
    Camera,
    #[route("/native")]
    Native,
    #[route("/player")]
    Player,
    #[route("/browser")]
    Browser,
    #[route("/shop")]
    Shop,
    #[route("/overlays")]
    Overlays,
    #[route("/otp")]
    Otp,
    #[route("/fields")]
    Fields,
    #[route("/empty")]
    Empty,
    #[route("/swipe")]
    Swipe,
    #[route("/wheel")]
    Wheel,
    #[route("/display")]
    Display,
    #[route("/conversation")]
    Conversation,
    #[route("/questionnaire")]
    Questionnaire,
    #[route("/cupertino")]
    Cupertino,
    /// A task's own screen.
    #[route("/task/:id")]
    Task { id: u64 },
}

/// The demo's routes. Home is the root: every other pattern begins with it, so every other
/// screen is a sub-route of it and going back from any of them lands on the task list.
pub(crate) fn router(demo: Rc<Demo>) -> GoRouter {
    GoRouter::new(Route::go_routes(move |_, state, route| {
        let demo = demo.clone();
        match route {
            Route::Home => HomePage { demo }.into_widget(),
            Route::Settings => SettingsPage { demo }.into_widget(),
            Route::Wizard => WizardPage { demo }.into_widget(),
            Route::Grid => GridPage { demo }.into_widget(),
            Route::Journal => JournalPage.into_widget(),
            Route::Charts => ChartsPage.into_widget(),
            Route::Data => DataPage.into_widget(),
            Route::Board => BoardPage.into_widget(),
            Route::Tour => TourPage.into_widget(),
            Route::Sheet => SheetPage.into_widget(),
            Route::Licenses => LicensesPage.into_widget(),
            Route::Video => VideoPage.into_widget(),
            Route::Camera => CameraPage.into_widget(),
            Route::Native => NativePage.into_widget(),
            Route::Player => PlayerPage.into_widget(),
            Route::Browser => BrowserPage.into_widget(),
            Route::Shop => ShopPage.into_widget(),
            Route::Overlays => OverlaysPage.into_widget(),
            Route::Otp => OtpPage.into_widget(),
            Route::Fields => FieldsPage.into_widget(),
            Route::Empty => EmptyPage.into_widget(),
            Route::Swipe => SwipePage.into_widget(),
            Route::Wheel => WheelPage.into_widget(),
            Route::Display => DisplayPage.into_widget(),
            Route::Conversation => ConversationPage.into_widget(),
            Route::Questionnaire => QuestionnairePage.into_widget(),
            Route::Cupertino => CupertinoPage.into_widget(),
            Route::Task { id } => TaskPage {
                demo,
                id,
                entering: state.entering(),
            }
            .into_widget(),
        }
    }))
}
