//! Caret events (roadmap 4.4). A dev tool for checking the caret thread; not shipped.
//!
//! Runs the app's caret thread (`spawn_caret_source`), prints every `CoreEvent` it publishes and
//! outlines the reported rectangle in its source's colour: red gui, green msaa, blue uia,
//! yellow focus rectangle. Rectangles are physical virtual-screen pixels, so an outline that
//! sits on the caret at 150% display scaling shows the conversion is right.
//!
//! Run: `cargo run -p cv-platform-win --example caret_events`. Ctrl+C stops it; it changes
//! nothing system-wide.

fn main() {
    #[cfg(windows)]
    events::run();
}

#[cfg(windows)]
#[path = "common/outline.rs"]
mod outline;

#[cfg(windows)]
mod events {
    use super::outline;
    use std::{sync::Arc, time::Instant};

    use cv_core::{CaretSource, CoreEvent, EventHub, ScreenRect};

    pub fn run() {
        // Physical pixels everywhere, as in the app.
        cv_platform_win::set_dpi_awareness();
        outline::spawn();

        let hub = Arc::new(EventHub::new());
        let events = hub.subscribe();
        let _caret = cv_platform_win::spawn_caret_source(hub);

        println!(
            "caret events: one line per event, rects are x,y wxh in physical screen pixels. \
             Outlines: red gui, green msaa, blue uia, yellow focus rect. Ctrl+C to stop."
        );
        let start = Instant::now();
        for event in events {
            let t = start.elapsed().as_secs_f64();
            match event {
                CoreEvent::CaretMoved { rect, source, app, .. } => {
                    println!("[{t:>8.3}s] moved {:<10} {}  {}", source.name(), fmt_rect(rect), app.exe);
                    let mut slots = [None; outline::SLOTS];
                    slots[slot(source)] = Some(rect);
                    outline::show(slots);
                }
                CoreEvent::CaretLost { app, .. } => {
                    println!("[{t:>8.3}s] lost                {}", app.exe);
                    outline::show([None; outline::SLOTS]);
                }
                other => println!("[{t:>8.3}s] {other:?}"),
            }
        }
    }

    fn slot(source: CaretSource) -> usize {
        match source {
            CaretSource::Gui => 0,
            CaretSource::Msaa => 1,
            CaretSource::Uia => 2,
            _ => 3,
        }
    }

    fn fmt_rect(r: ScreenRect) -> String {
        format!("{},{} {}x{}", r.left, r.top, r.width, r.height)
    }
}
