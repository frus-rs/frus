use frus::{
    Align, Callback, Column, Container, Expanded, FontWeight, GestureDetector, Justify, Row, Widget,
};

use crate::{
    common::{
        colors::{black, grey, primary_color, white},
        icons::{icon, Glyph},
        ui::{filled, outlined, text, FULL},
    },
    models::calendar::{days_in, weekday},
};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A month's calendar, the chosen day in a green circle.
pub(crate) fn calendar(
    year: i32,
    month: u32,
    selected: Option<u32>,
    on_month: impl Fn(i32) -> Callback,
    on_day: impl Fn(u32) -> Callback,
) -> Box<dyn Widget> {
    let arrow = |glyph, by: i32| {
        GestureDetector::new(
            Container::new()
                .width(32.0)
                .height(32.0)
                .alignment(frus::Alignment::CENTER)
                .child(icon(glyph, 16.0, grey())),
        )
        .on_tap(on_month(by))
    };
    let header = Row::new()
        .main_axis_alignment(Justify::SpaceBetween)
        .cross_axis_alignment(Align::Center)
        .child(arrow(Glyph::ChevronLeft, -1))
        .child(text(
            format!("{} {year}", MONTHS[month as usize - 1]),
            14.0,
            20.0,
            black(),
            FontWeight::SemiBold,
        ))
        .child(arrow(Glyph::ChevronRight, 1));

    let cell = |child: Box<dyn Widget>| {
        Expanded::new(
            Container::new()
                .height(20.0)
                .alignment(frus::Alignment::CENTER)
                .child(child),
        )
    };

    let mut grid = Column::new()
        .spacing(4.0)
        .cross_axis_alignment(Align::Stretch);
    let mut names = Row::new();
    for name in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] {
        names = names.child(cell(Box::new(text(
            name,
            12.0,
            16.0,
            grey(),
            FontWeight::Regular,
        ))));
    }
    grid = grid.child(names);

    let first = weekday(year, month, 1);
    let count = days_in(year, month);
    let mut day = 1;
    let mut slot = 0;
    while day <= count {
        let mut week = Row::new();
        for _ in 0..7 {
            if slot < first || day > count {
                week = week.child(cell(Box::new(Container::new())));
            } else {
                let chosen = selected == Some(day);
                let label = text(
                    day.to_string(),
                    12.0,
                    16.0,
                    if chosen { white() } else { grey() },
                    if chosen {
                        FontWeight::SemiBold
                    } else {
                        FontWeight::Regular
                    },
                );
                let mut mark = Container::new()
                    .width(22.0)
                    .height(22.0)
                    .alignment(frus::Alignment::CENTER);
                if chosen {
                    mark = mark.decoration(filled(primary_color(), FULL));
                }
                week = week.child(cell(Box::new(
                    GestureDetector::new(mark.child(label)).on_tap(on_day(day)),
                )));
                day += 1;
            }
            slot += 1;
        }
        grid = grid.child(week);
    }

    Box::new(
        Container::new()
            .decoration(outlined(8.0))
            .padding(17.0)
            .child(
                Column::new()
                    .spacing(16.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(header)
                    .child(grid),
            ),
    )
}
