//! Dev check for the speech thread (roadmap 8.8b, ADR 0013). Not shipped.
//!
//! `CV_TTS_LOG=1 cargo run -p cv-tts --example speech_thread_probe -- [--stop-after-ms N] [--volume 0-100] [--rate -10..10]`
//!
//! Starts the speech thread with speech on and says a long sentence. Press Enter (or wait
//! `--stop-after-ms`) to silence it, then it says a short sentence to show speech still works, and
//! shuts down. With `CV_TTS_LOG=1` the thread prints a `[tts]` line with a time for each step:
//! the silence request, how long the engine's stop call took, and each finished report. Compare
//! the stop with what you hear; it should end within about 100 ms.

use std::io::BufRead;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cv_core::{EventHub, Kind, Utterance, new_shared};

const LONG: &str = "This is a long sentence that goes on and on, so that there is plenty of speech \
left to stop. It keeps going for several seconds, and the only thing that should end it early is \
the silence request that this probe makes. Press Enter whenever you like, and listen for how \
quickly the voice stops after you do.";

struct Args {
    stop_after_ms: Option<u64>,
    volume: u32,
    rate: i32,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { stop_after_ms: None, volume: 100, rate: 0 };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--stop-after-ms" => {
                args.stop_after_ms = Some(value()?.parse().map_err(|_| "--stop-after-ms: not a number")?)
            }
            "--volume" => args.volume = value()?.parse().map_err(|_| "--volume: not a number")?,
            "--rate" => args.rate = value()?.parse().map_err(|_| "--rate: not a number")?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[tts] {e}");
            std::process::exit(2);
        }
    };
    let state = new_shared();
    {
        let mut s = state.write();
        s.tts_enabled = true;
        s.tts_volume = args.volume;
        s.tts_rate = args.rate;
    }
    let events = EventHub::new();
    let speech = cv_tts::spawn_speech(state.clone(), &events);
    // Let the engines start (the thread makes them on its first pass).
    sleep(Duration::from_millis(500));

    speech.say(Utterance::new(Kind::Test, LONG));
    let said = Instant::now();
    match args.stop_after_ms {
        Some(ms) => {
            println!("[probe] silencing after {ms} ms");
            sleep(Duration::from_millis(ms));
        }
        None => {
            println!("[probe] speaking; press Enter to silence");
            let _ = std::io::stdin().lock().lines().next();
        }
    }
    println!("[probe] {:>7} ms after say: silence()", said.elapsed().as_millis());
    speech.silence();

    // Speech must still work after a silence.
    sleep(Duration::from_millis(1500));
    speech.say(Utterance::new(Kind::Test, "That was the end of the first sentence."));
    sleep(Duration::from_secs(4));

    // Turning speech off stops it and releases the engines.
    speech.say(Utterance::new(Kind::Test, "Now I am turned off in the middle of speaking, so you should not hear all of this."));
    sleep(Duration::from_millis(800));
    state.write().tts_enabled = false;
    println!("[probe] speech off");
    speech.wake();
    sleep(Duration::from_secs(2));
    speech.shutdown();
    println!("[probe] done");
}
