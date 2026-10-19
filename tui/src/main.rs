//! Terminal view of the river: the simulation runs natively and is drawn
//! with ratatui as half-block pixels, two to a cell.
//!
//! cargo run --release -p meander-tui -- [seed]
//!
//! Without `seed` the run draws one from the clock and shows it.

mod render;

use meander_core::{Params, Sim, YEARS_PER_UNIT};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use render::{Picture, View};
use std::time::{Duration, Instant};

/// Simulated time run before the year count starts: 2,000 years, as on the
/// page.
const WARMUP: f64 = 800.0;
/// Lateral noise of the starting line, in channel widths, as on the page.
const NOISE: f64 = 0.5;
/// Height of the world shown, in channel widths: the 45-width valley, the
/// walls' wander and a margin of upland either side.
const VIEW_HEIGHT: f64 = 70.0;
/// Speeds the keys step through, in years a second, within the page's
/// range of 1.25 to 150.
const SPEEDS: [f64; 8] = [1.25, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0, 150.0];
/// Index in `SPEEDS` of the page's default speed.
const SPEED: usize = 2;
/// Time between frames.
const FRAME: Duration = Duration::from_millis(33);
/// Time a frame may spend stepping. Simulated time the river cannot cover
/// in it is dropped, so a slow machine runs below the set speed.
const STEP_BUDGET: Duration = Duration::from_millis(25);

struct App {
    sim: Sim,
    seed: u64,
    /// Size of the shown world, in world units.
    world: [f64; 2],
    /// Index in `SPEEDS`.
    speed: usize,
    paused: bool,
    /// Simulated time owed to the loop but not yet stepped.
    owed: f64,
    picture: Picture,
}

impl App {
    /// A river sized to fill a terminal `cols` by `rows` cells, less the
    /// status line.
    fn new(seed: u64, cols: u16, rows: u16) -> Self {
        let params = Params::default();
        let (pw, ph) = (
            cols.max(1) as f64,
            (2 * rows.saturating_sub(1)).max(2) as f64,
        );
        let h = VIEW_HEIGHT * params.width;
        let world = [h * pw / ph, h];
        let mut sim = Sim::new(params, [1.0, 1.0], seed, 0.0);
        sim.reset_view(world[0], world[1], seed, NOISE);
        App {
            sim,
            seed,
            world,
            speed: SPEED,
            paused: false,
            owed: 0.0,
            picture: Picture::default(),
        }
    }

    fn warmup_steps(&self) -> u64 {
        (WARMUP / self.sim.params.dt).round() as u64
    }

    fn warming(&self) -> bool {
        self.sim.steps < self.warmup_steps()
    }

    /// Step for at most `STEP_BUDGET`: through the warm-up as fast as it
    /// goes, then at the set speed for the `elapsed` wall time.
    fn advance(&mut self, elapsed: f64) {
        let start = Instant::now();
        if self.warming() {
            while self.warming() && start.elapsed() < STEP_BUDGET {
                self.sim.step();
            }
            return;
        }
        if self.paused {
            return;
        }
        let dt = self.sim.params.dt;
        self.owed += SPEEDS[self.speed] / YEARS_PER_UNIT * elapsed;
        while self.owed >= dt && start.elapsed() < STEP_BUDGET {
            self.sim.step();
            self.owed -= dt;
        }
        self.owed = self.owed.min(dt);
    }

    fn status(&self) -> Line<'static> {
        let epoch = self.warmup_steps();
        let dim = Style::new().fg(Color::Rgb(150, 146, 130));
        let state = if self.warming() {
            format!(
                " warming up {:3.0}%",
                100.0 * self.sim.steps as f64 / epoch as f64
            )
        } else {
            let years = (self.sim.steps - epoch) as f64 * self.sim.params.dt * YEARS_PER_UNIT;
            format!(
                " year {:>6}{}",
                thousands(years.round() as u64),
                if self.paused { "  paused" } else { "" }
            )
        };
        Line::from(vec![
            Span::raw(state),
            Span::styled(
                format!("   {} years/s   seed {}   ", SPEEDS[self.speed], self.seed),
                dim,
            ),
            Span::raw("space"),
            Span::styled(" pause  ", dim),
            Span::raw("+/-"),
            Span::styled(" speed  ", dim),
            Span::raw("n"),
            Span::styled(" new river  ", dim),
            Span::raw("q"),
            Span::styled(" quit", dim),
        ])
    }
}

/// `n` with commas between groups of three digits.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn clock_seed() -> u64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    now.map_or(1, |d| (d.as_micros() % 1_000_000) as u64)
}

fn run(terminal: &mut DefaultTerminal, seed: u64) -> std::io::Result<()> {
    let size = terminal.size()?;
    let mut app = App::new(seed, size.width, size.height);
    let mut last = Instant::now();
    loop {
        terminal.draw(|f| {
            let [map, status] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(f.area());
            let view = View::fit(&app.sim, app.world, map.width, 2 * map.height);
            app.picture.paint(&app.sim, &view);
            f.render_widget(&app.picture, map);
            f.render_widget(app.status(), status);
        })?;
        // The warm-up steps every frame it can, so it waits only for keys
        // already pressed.
        let next = if app.warming() {
            Instant::now()
        } else {
            last + FRAME
        };
        loop {
            let wait = next.saturating_duration_since(Instant::now());
            if !event::poll(wait)? {
                break;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char(' ') => app.paused = !app.paused,
                KeyCode::Char('+' | '=') => app.speed = (app.speed + 1).min(SPEEDS.len() - 1),
                KeyCode::Char('-' | '_') => app.speed = app.speed.saturating_sub(1),
                KeyCode::Char('n') => {
                    let size = terminal.size()?;
                    app = App::new(clock_seed(), size.width, size.height);
                }
                _ => {}
            }
        }
        let now = Instant::now();
        app.advance((now - last).as_secs_f64());
        last = now;
    }
}

fn main() -> std::io::Result<()> {
    let seed = std::env::args()
        .nth(1)
        .map_or_else(clock_seed, |s| s.parse().expect("usage: meander [seed]"));
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, seed);
    ratatui::restore();
    result
}
