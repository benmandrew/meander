//! Meandering river centreline simulation.
//!
//! The river is a polyline flowing from index 0 (upstream) to the last index
//! (downstream). Each step resamples it, migrates every interior point along
//! its normal at a rate given by the Howard–Knutson (1984) upstream-weighted
//! curvature kernel, smooths, and cuts off any loop whose neck has closed.

pub mod farm;
pub mod field;
pub mod geom;
pub mod ips;
pub mod land;
pub mod mills;
pub mod ponds;
pub mod rng;
pub mod tracks;
pub mod tributary;
pub mod walls;
pub mod wood;

use field::{Erodibility, Ground, Overbank, value_noise};
use geom::{P, curvature, dist, resample, right_normal};
use land::Land;
pub use mills::Mill;
use rng::Rng;
use tracks::Tracks;
pub use tributary::Tributary;
use walls::Walls;
pub use wood::Jam;

/// Point spacing as a fraction of the channel width.
pub const SPACING: f64 = 0.5;

#[derive(Clone, Debug)]
pub struct Params {
    /// Reference channel width `w`, in world units. Sets the point spacing
    /// and the unit of every length in channel widths. The local width
    /// varies about it; see [`Sim::widths`].
    pub width: f64,
    /// Time over which bar push brings the local width back to `width`
    /// once the banks stop moving. 0 holds the width at `width`.
    pub width_time: f64,
    /// Rate at which the inner bank advances as the point bar grows, as a
    /// fraction of the rate at which a bank of erodibility 1 retreats. The
    /// channel widens where the outer bank erodes faster than this, and
    /// narrows where it erodes slower.
    pub bar_push: f64,
    /// Erodibility `E`: migration speed, in channel widths per unit time per
    /// unit of dimensionless rate.
    pub erodibility: f64,
    /// Weight `Ω` on local curvature (negative in Howard–Knutson).
    pub omega: f64,
    /// Weight `Γ` on the upstream-averaged curvature.
    pub gamma: f64,
    /// Upstream decay rate `α`, per channel width.
    pub alpha: f64,
    /// Bend radius, in channel widths, at which migration is fastest; tighter
    /// bends migrate more slowly. 0 disables the saturation.
    pub peak_radius: f64,
    /// Exponent of the channel slope in the migration rate. The channel
    /// falls as far as the valley over a reach, so its slope is the valley
    /// slope divided by the reach's sinuosity, and a winding reach migrates
    /// more slowly than a straight one. Near-bank velocity goes as the
    /// square root of slope at a fixed depth (Chézy), so 0.5. 0 disables.
    pub slope_exponent: f64,
    /// Sinuosity of a reach at which the slope leaves the migration rate
    /// unchanged. Straighter reaches migrate faster and more winding ones
    /// more slowly.
    pub slope_sinuosity: f64,
    pub dt: f64,
    /// Laplacian smoothing strength per step. It straightens kinks from
    /// point to point, which the filtered local term of the migration rate
    /// leaves half as strongly.
    pub smoothing: f64,
    /// Cutoff distance `d_cut`, in channel widths.
    pub cutoff_dist: f64,
    /// Minimum index separation `K` for a cutoff pair.
    pub cutoff_min_sep: usize,
    /// Per-step displacement limit, as a fraction of the point spacing.
    pub max_step: f64,
    /// Simulation time an oxbow lake takes to fill with no sediment from the
    /// channel. Lakes near the channel fill faster.
    pub oxbow_lifetime: f64,
    /// Fraction of a lake filled per unit time with bedload through an
    /// entrance still at the channel, at a diversion angle of 0. It falls
    /// with the cosine of the angle and with the entrance's distance from
    /// the channel over `levee_length`. 0 disables.
    pub bedload_rate: f64,
    /// Simulation time a filled oxbow stays as a scar before it is dropped.
    pub scar_lifetime: f64,
    /// Mean time between the sets of scroll bars, the ridges the river
    /// leaves along its inner banks at high water. They come as a Poisson
    /// process, so the ridges on a bar are spaced unevenly. 0 disables.
    pub scroll_time: f64,
    /// Simulation time a scroll bar lasts before it is dropped. 0 disables
    /// scroll bars.
    pub scroll_lifetime: f64,
    /// Random lateral perturbation added each step, in channel widths per
    /// square root of time. Bends drift downstream and leave the domain, so
    /// without this the river eventually straightens.
    pub jitter: f64,
    /// Correlation length of the jitter in space, in channel widths: its
    /// lattice is twice this apart.
    pub jitter_scale: f64,
    /// Time over which the jitter at a point turns, so the river drifts
    /// steadily between frames and does not shake from step to step.
    pub jitter_time: f64,
    /// Width of the valley the river may migrate within, centred on the
    /// domain, in channel widths. 0 allows the whole domain. The walls
    /// (see [`Sim::walls`]) wander outwards of it, never inwards.
    pub valley_width: f64,
    /// Width of the band along each domain edge over which erodibility
    /// tapers to zero, in channel widths.
    pub edge_margin: f64,
    /// Valley width at each of [`Sim::pinches`], in channel widths. The
    /// river crosses a pinch near the valley axis, so a renderer can put the
    /// window edges there and no bend straddles them. 0 disables.
    pub pinch_width: f64,
    /// Distance either side of a pinch over which the valley widens back to
    /// `valley_width`, in channel widths.
    pub pinch_length: f64,
    /// Strength of the erodibility noise: 0 is uniform, 1 ranges from
    /// near zero to double.
    pub heterogeneity: f64,
    /// Erodibility multiplier inside filled oxbows (clay plugs); 1 disables.
    pub clay_factor: f64,
    /// Erodibility multiplier inside an oxbow lake when it is cut off. The
    /// bank breaks through quickly into open water, and the multiplier moves
    /// to `clay_factor` as the lake fills over `oxbow_lifetime`.
    pub lake_factor: f64,
    /// Erodibility multiplier on ground the channel has just left, as bare
    /// point-bar sand. It moves to `shrub_factor` as willow and shrub bind
    /// the sand over `vegetation_time`; 1 disables.
    pub bar_factor: f64,
    /// Simulation time vegetation takes to bind a point bar.
    pub vegetation_time: f64,
    /// Erodibility multiplier on a point bar once shrub has bound it. It
    /// moves to 1, the mature forest of the old floodplain, as forest grows
    /// over `forest_time`; 1 disables.
    pub shrub_factor: f64,
    /// Simulation time shrub takes to grow into mature forest.
    pub forest_time: f64,
    /// The channel takes up an open oxbow lake whose banks it meets at two
    /// places when the route through the lake is shorter than the channel
    /// between them by more than this factor, and abandons the channel's
    /// route as a new oxbow. The steeper route draws the flow, as an
    /// avulsion takes up an abandoned channel. A lake filled past
    /// [`CAPTURE_FILL`] resists. 0 disables.
    pub capture_ratio: f64,
    /// Chance per unit time that flood water cuts a *chute* across the
    /// bare point bar of a long bend, as a channel through a swale, and
    /// abandons the bend as an oxbow, at a sediment supply of 1. Clearance
    /// raises it with the supply, as sediment-laden floods build bars and
    /// cut chutes. Bars that vegetation has bound do not cut; see
    /// [`Sim::chute`]. 0 disables.
    pub chute_rate: f64,
    /// Overbank deposit laid at the bank per unit time, in channel depths,
    /// before it thins towards the flood level: the mean of what the
    /// river's floods lay, spread evenly over time. 0 disables.
    pub overbank_rate: f64,
    /// Distance over which overbank deposits thin by a factor of e away
    /// from the bank, in channel widths.
    pub levee_length: f64,
    /// Flood level above fresh point-bar sand, in channel depths: the
    /// highest ground overbank deposits can build. A bank raised by h
    /// erodes 1 / (1 + h) as fast.
    pub levee_height: f64,
    /// Overbank deposit, in channel depths, at which vegetation grows
    /// fastest. Ground that high water seldom covers grows shrub and forest
    /// sooner than bare bar sand beside the water; 0 grows every cell at the
    /// rate `vegetation_time` and `forest_time` assume.
    pub vegetation_height: f64,
    /// Height a scroll bar's ridge raises the ground, in channel depths, so
    /// vegetation takes to the ridges first. 0 disables.
    pub scroll_height: f64,
    /// Area a landholding settles to, in square channel widths. One that
    /// grows past twice this divides between heirs; one that shrinks below
    /// 0.3 of it is absorbed by its neighbours. See [`land`].
    pub holding_area: f64,
    /// Mean time between one landholding's reorganisations, when its
    /// boundaries can move. Holdings reorganise one at a time.
    pub reorganise_time: f64,
    /// Growth at which grass has taken a point bar well enough to graze or
    /// mow, after which a landholding can take the ground. Below
    /// `vegetation_time`, so holdings follow the channel onto bars long
    /// before shrub binds them.
    pub grazing_time: f64,
    /// Whether landholdings raise farmsteads of small buildings (see
    /// [`farm`]). The buildings read the land and change nothing else.
    pub farmsteads: bool,
    /// Chance that an heir's farmstead goes up beside the parent's when a
    /// holding divides, making a hamlet. Takes effect only with
    /// `farmsteads` on.
    pub hamlet_chance: f64,
    /// Erodibility multiplier on wholly coarse ground: the gravel of a
    /// tributary fan, or colluvium slumped from a valley wall. 1 disables.
    pub coarse_factor: f64,
    /// Time the upland clearance begins. Fine sediment from cleared
    /// slopes multiplies overbank deposition, the filling of lakes and the
    /// growth of tributary fans by [`Sim::supply`].
    pub clearance_start: f64,
    /// Time over which the supply rises to `clearance_supply`.
    pub clearance_time: f64,
    /// Sediment supply once the upland is cleared, as a multiple of the
    /// wooded upland's. 1 disables.
    pub clearance_supply: f64,
    /// Jams of large wood recruited per channel width of mature forest
    /// bank eroded. Farmland gives [`FARM_WOOD`] of that. 0 disables.
    pub wood_rate: f64,
    /// Time a jam of full size lasts before it rots or breaks up; smaller
    /// jams last less.
    pub jam_lifetime: f64,
    /// Push a jam of full size gives the flow away from its bank, added to
    /// the dimensionless migration rate at the jam and falling to 0
    /// [`JAM_REACH`] widths along the channel either side.
    pub jam_push: f64,
    /// Rate at which a bank pressing against a valley wall erodes it back,
    /// as a fraction of the bank's migration demand. 0 disables.
    pub bluff_rate: f64,
    /// Radius of the fan a tributary builds at the mouth of the deepest
    /// combe, in channel widths. Shallower combes build smaller fans, and
    /// combes under [`FAN_MIN_DEPTH`] carry no tributary. 0 disables.
    pub fan_radius: f64,
    /// Time a fan takes to build back what the channel has trimmed, at a
    /// sediment supply of 1.
    pub fan_time: f64,
    /// Increase in the migration rate at a confluence from a tributary of
    /// the deepest combe, falling to nothing [`TRIBUTARY_REACH`] widths
    /// downstream: the tributary's bedload feeds the bars.
    pub tributary_boost: f64,
    /// Depth of lake a tributary of the deepest combe fills per unit time
    /// at the head of the delta it builds where it enters a lake, as a
    /// fraction of a fresh lake's depth, at a sediment supply of 1. 0
    /// disables.
    pub delta_rate: f64,
    /// Expected moves per unit time of each tributary's course, from a
    /// point on its fan to a new route through low ground. 0 disables.
    pub stream_shift_rate: f64,
    /// Mills founded per unit time along the whole river, by holdings on
    /// straight reaches beside their ground. 0 disables.
    pub mill_rate: f64,
    /// Least distance between two working mills, in channel widths.
    pub mill_gap: f64,
    /// Fraction by which a mill's weir slows migration just upstream of
    /// it, falling to 0 at `pond_length`.
    pub weir_backwater: f64,
    /// Length of the pond a weir holds back, in channel widths.
    pub pond_length: f64,
    /// Derive `omega`, `gamma` and `alpha` from the hydraulic inputs below
    /// with the Ikeda–Parker–Sawai model (see [`ips`]).
    pub physics: bool,
    /// Friction coefficient `C_f`.
    pub friction: f64,
    /// Width to depth ratio `w/H`.
    pub aspect: f64,
    /// Froude number `U/√(gH)`.
    pub froude: f64,
    /// Scour factor `A`: transverse bed slope per unit curvature.
    pub scour: f64,
}

impl Default for Params {
    fn default() -> Self {
        // A lowland sand-bed river: friction and Froude number in the
        // ranges measured on such rivers, a width 30 times the depth, and
        // the scour factor from Ikeda (1981). The linear theory's fastest
        // growing bend then has a wavelength of 2π / (α √(√Γ − 1)) ≈ 12
        // channel widths, inside the 10 to 14 seen on real rivers.
        let (friction, aspect, froude, scour) = (0.01, 30.0, 0.3, 2.89);
        let (omega, gamma, alpha) = ips::coefficients(friction, aspect, froude, scour);
        Self {
            width: 8.0,
            width_time: WIDTH_TIME,
            bar_push: BAR_PUSH,
            erodibility: 1.0,
            omega,
            gamma,
            alpha,
            peak_radius: 2.5,
            slope_exponent: SLOPE_EXPONENT,
            slope_sinuosity: SLOPE_SINUOSITY,
            dt: 0.1,
            smoothing: 0.045,
            cutoff_dist: 1.0,
            cutoff_min_sep: 20,
            max_step: 0.15,
            oxbow_lifetime: OXBOW_LIFETIME,
            bedload_rate: BEDLOAD_RATE,
            scar_lifetime: 1600.0,
            scroll_time: SCROLL_TIME,
            scroll_lifetime: SCROLL_LIFETIME,
            jitter: 0.05,
            jitter_scale: 3.0,
            // 5 years, a second at the page's default speed.
            jitter_time: 2.0,
            valley_width: 45.0,
            edge_margin: 8.0,
            pinch_width: 6.0,
            pinch_length: 30.0,
            heterogeneity: 0.5,
            clay_factor: 0.35,
            lake_factor: 3.0,
            bar_factor: BAR_FACTOR,
            vegetation_time: VEGETATION_TIME,
            shrub_factor: SHRUB_FACTOR,
            forest_time: FOREST_TIME,
            capture_ratio: CAPTURE_RATIO,
            chute_rate: CHUTE_RATE,
            overbank_rate: OVERBANK_RATE,
            levee_length: LEVEE_LENGTH,
            levee_height: LEVEE_HEIGHT,
            vegetation_height: VEGETATION_HEIGHT,
            scroll_height: SCROLL_HEIGHT,
            holding_area: HOLDING_AREA,
            reorganise_time: REORGANISE_TIME,
            grazing_time: GRAZING_TIME,
            farmsteads: true,
            hamlet_chance: HAMLET_CHANCE,
            coarse_factor: COARSE_FACTOR,
            clearance_start: CLEARANCE_START,
            clearance_time: CLEARANCE_TIME,
            clearance_supply: CLEARANCE_SUPPLY,
            wood_rate: WOOD_RATE,
            jam_lifetime: JAM_LIFETIME,
            jam_push: JAM_PUSH,
            bluff_rate: BLUFF_RATE,
            fan_radius: FAN_RADIUS,
            fan_time: FAN_TIME,
            tributary_boost: TRIBUTARY_BOOST,
            delta_rate: DELTA_RATE,
            stream_shift_rate: STREAM_SHIFT_RATE,
            mill_rate: MILL_RATE,
            mill_gap: MILL_GAP,
            weir_backwater: WEIR_BACKWATER,
            pond_length: POND_LENGTH,
            physics: true,
            friction,
            aspect,
            froude,
            scour,
        }
    }
}

impl Params {
    /// Names accepted by [`Params::get`] and [`Params::set`].
    pub const NAMES: &[&str] = &[
        "width",
        "width_time",
        "bar_push",
        "erodibility",
        "omega",
        "gamma",
        "alpha",
        "peak_radius",
        "slope_exponent",
        "slope_sinuosity",
        "dt",
        "smoothing",
        "cutoff_dist",
        "cutoff_min_sep",
        "max_step",
        "oxbow_lifetime",
        "bedload_rate",
        "scar_lifetime",
        "scroll_time",
        "scroll_lifetime",
        "jitter",
        "jitter_scale",
        "jitter_time",
        "valley_width",
        "edge_margin",
        "pinch_width",
        "pinch_length",
        "heterogeneity",
        "clay_factor",
        "lake_factor",
        "bar_factor",
        "vegetation_time",
        "shrub_factor",
        "forest_time",
        "capture_ratio",
        "chute_rate",
        "overbank_rate",
        "levee_length",
        "levee_height",
        "vegetation_height",
        "scroll_height",
        "holding_area",
        "reorganise_time",
        "grazing_time",
        "farmsteads",
        "hamlet_chance",
        "coarse_factor",
        "clearance_start",
        "clearance_time",
        "clearance_supply",
        "wood_rate",
        "jam_lifetime",
        "jam_push",
        "bluff_rate",
        "fan_radius",
        "fan_time",
        "tributary_boost",
        "delta_rate",
        "stream_shift_rate",
        "mill_rate",
        "mill_gap",
        "weir_backwater",
        "pond_length",
        "physics",
        "friction",
        "aspect",
        "froude",
        "scour",
    ];

    fn field(&mut self, name: &str) -> Option<&mut f64> {
        Some(match name {
            "width" => &mut self.width,
            "width_time" => &mut self.width_time,
            "bar_push" => &mut self.bar_push,
            "erodibility" => &mut self.erodibility,
            "omega" => &mut self.omega,
            "gamma" => &mut self.gamma,
            "alpha" => &mut self.alpha,
            "peak_radius" => &mut self.peak_radius,
            "slope_exponent" => &mut self.slope_exponent,
            "slope_sinuosity" => &mut self.slope_sinuosity,
            "dt" => &mut self.dt,
            "smoothing" => &mut self.smoothing,
            "cutoff_dist" => &mut self.cutoff_dist,
            "max_step" => &mut self.max_step,
            "oxbow_lifetime" => &mut self.oxbow_lifetime,
            "bedload_rate" => &mut self.bedload_rate,
            "scar_lifetime" => &mut self.scar_lifetime,
            "scroll_time" => &mut self.scroll_time,
            "scroll_lifetime" => &mut self.scroll_lifetime,
            "jitter" => &mut self.jitter,
            "jitter_scale" => &mut self.jitter_scale,
            "jitter_time" => &mut self.jitter_time,
            "valley_width" => &mut self.valley_width,
            "edge_margin" => &mut self.edge_margin,
            "pinch_width" => &mut self.pinch_width,
            "pinch_length" => &mut self.pinch_length,
            "heterogeneity" => &mut self.heterogeneity,
            "clay_factor" => &mut self.clay_factor,
            "lake_factor" => &mut self.lake_factor,
            "bar_factor" => &mut self.bar_factor,
            "vegetation_time" => &mut self.vegetation_time,
            "shrub_factor" => &mut self.shrub_factor,
            "forest_time" => &mut self.forest_time,
            "capture_ratio" => &mut self.capture_ratio,
            "chute_rate" => &mut self.chute_rate,
            "overbank_rate" => &mut self.overbank_rate,
            "levee_length" => &mut self.levee_length,
            "levee_height" => &mut self.levee_height,
            "vegetation_height" => &mut self.vegetation_height,
            "scroll_height" => &mut self.scroll_height,
            "holding_area" => &mut self.holding_area,
            "reorganise_time" => &mut self.reorganise_time,
            "grazing_time" => &mut self.grazing_time,
            "hamlet_chance" => &mut self.hamlet_chance,
            "coarse_factor" => &mut self.coarse_factor,
            "clearance_start" => &mut self.clearance_start,
            "clearance_time" => &mut self.clearance_time,
            "clearance_supply" => &mut self.clearance_supply,
            "wood_rate" => &mut self.wood_rate,
            "jam_lifetime" => &mut self.jam_lifetime,
            "jam_push" => &mut self.jam_push,
            "bluff_rate" => &mut self.bluff_rate,
            "fan_radius" => &mut self.fan_radius,
            "fan_time" => &mut self.fan_time,
            "tributary_boost" => &mut self.tributary_boost,
            "delta_rate" => &mut self.delta_rate,
            "stream_shift_rate" => &mut self.stream_shift_rate,
            "mill_rate" => &mut self.mill_rate,
            "mill_gap" => &mut self.mill_gap,
            "weir_backwater" => &mut self.weir_backwater,
            "pond_length" => &mut self.pond_length,
            "friction" => &mut self.friction,
            "aspect" => &mut self.aspect,
            "froude" => &mut self.froude,
            "scour" => &mut self.scour,
            _ => return None,
        })
    }

    /// Flags read as 0 or 1.
    pub fn get(&self, name: &str) -> Option<f64> {
        match name {
            "cutoff_min_sep" => Some(self.cutoff_min_sep as f64),
            "physics" => Some(if self.physics { 1.0 } else { 0.0 }),
            "farmsteads" => Some(if self.farmsteads { 1.0 } else { 0.0 }),
            _ => self.clone().field(name).map(|v| *v),
        }
    }

    /// Set a parameter by name; a flag is set by any non-zero value. With
    /// `physics` on, `omega`, `gamma` and `alpha` are re-derived after every
    /// set, so setting them directly has no effect. Returns false for an
    /// unknown name.
    pub fn set(&mut self, name: &str, value: f64) -> bool {
        match name {
            "cutoff_min_sep" => self.cutoff_min_sep = value.max(2.0) as usize,
            "physics" => self.physics = value != 0.0,
            "farmsteads" => self.farmsteads = value != 0.0,
            _ => match self.field(name) {
                Some(v) => *v = value,
                None => return false,
            },
        }
        if self.physics {
            (self.omega, self.gamma, self.alpha) =
                ips::coefficients(self.friction, self.aspect, self.froude, self.scour);
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct Oxbow {
    pub points: Vec<P>,
    /// Width of the channel at each point when it was cut off, in
    /// multiples of [`Params::width`].
    pub widths: Vec<f64>,
    pub born: f64,
    /// Arc length when the oxbow was cut off. Pieces split from it keep
    /// it, so the drawn lake plugs at a rate that does not jump when the
    /// channel trims the oxbow.
    pub length: f64,
    /// Arc length from the first point of the oxbow as cut off to the
    /// first point of this piece, so only the original ends plug.
    pub offset: f64,
    /// Fraction of the lake filled, in [0, 1].
    pub fill: f64,
    /// Fill that silt and bedload have brought into the lake and that has
    /// yet to settle into `fill`, which it does over about `SETTLE_TIME`.
    pub settling: f64,
    /// Time the lake filled and became a scar, or infinity.
    pub filled_at: f64,
    /// Cosine of the diversion angle between the new channel and the
    /// oxbow's entrance at the cut, or 0 past a right angle. Bedload enters
    /// a lake cut at a small angle (Constantine et al. 2010).
    pub entry: f64,
}

impl Oxbow {
    /// A fresh lake along the points, with the channel's widths there,
    /// cut off at `born`.
    pub fn lake(points: Vec<P>, widths: Vec<f64>, born: f64, entry: f64) -> Self {
        let length = geom::length(&points);
        Self {
            points,
            widths,
            born,
            length,
            offset: 0.0,
            fill: 0.0,
            settling: 0.0,
            filled_at: f64::INFINITY,
            entry,
        }
    }

    pub fn is_lake(&self) -> bool {
        self.filled_at.is_infinite()
    }
}

/// A scroll bar: the ridge high water leaves along the inner bank of a
/// migrating bend, as a polyline.
#[derive(Clone, Debug)]
pub struct Scroll {
    pub points: Vec<P>,
    pub born: f64,
}

pub struct Sim {
    pub params: Params,
    /// Domain is `[0, bounds[0]] × [0, bounds[1]]`.
    pub bounds: P,
    /// Positions along x where the valley narrows to `pinch_width`.
    pub pinches: Vec<f64>,
    /// Span along x of the window the page shows, unbounded unless
    /// [`Sim::reset_view`] sets it. The holdings, and with them the
    /// farmsteads, churches, fishponds, tracks and mills, keep within
    /// [`WINDOW_MARGIN`] of it, since nothing beyond it is drawn.
    pub window: P,
    /// The valley walls, drawn from the seed at each reset.
    pub walls: Walls,
    pub centerline: Vec<P>,
    /// Local channel width at each centreline point, in multiples of
    /// [`Params::width`]. The outer bank retreats at the migration rate
    /// times its erodibility, the inner bank advances at [`Params::bar_push`]
    /// times the rate, and the width changes by the difference while it
    /// relaxes to 1 over [`Params::width_time`] (after the bank-pull and
    /// bar-push model of Parker et al. 2011).
    pub widths: Vec<f64>,
    /// Channel slope at each centreline point as a fraction of the valley
    /// slope: the inverse of the sinuosity of the reach [`SLOPE_REACH`]
    /// widths either side. Set by [`Sim::migration_rate`].
    pub slope: Vec<f64>,
    pub oxbows: Vec<Oxbow>,
    /// Counts changes to the oxbow polylines, but not to their fill, so a
    /// renderer can skip rebuilding what depends only on their shape.
    pub oxbow_version: u64,
    pub scrolls: Vec<Scroll>,
    /// Counts changes to `scrolls`, so a renderer can skip redrawing them.
    pub scroll_version: u64,
    pub erodibility: Erodibility,
    /// Landholdings on the floodplain, on the erodibility grid. They read
    /// the river and never change it.
    pub land: Land,
    pub time: f64,
    pub steps: u64,
    pub cutoffs: u64,
    /// Oxbow lakes the channel has taken up (included in `cutoffs`).
    pub captures: u64,
    /// Chute cutoffs (included in `cutoffs`).
    pub chutes: u64,
    /// The next point the chute scan starts from.
    chute_at: usize,
    /// Jams of large wood against the banks.
    pub jams: Vec<Jam>,
    /// Tributaries entering from the deeper combes, with their fans.
    pub tributaries: Vec<Tributary>,
    /// Mills, working and ruined.
    pub mills: Vec<Mill>,
    /// Tracks between the farmsteads, and their fords.
    pub tracks: Tracks,
    /// The parish boundary: the centreline as it ran at [`BOUNDARY_TIME`],
    /// which stays where it was set as the channel moves on. Empty until
    /// then.
    pub boundary: Vec<P>,
    /// Counts changes to `mills`.
    pub mill_version: u64,
    /// The `mill_version` the holdings last read the working mills at.
    land_mills: u64,
    /// The tributaries' course versions when last handed to `land`.
    land_streams: u64,
    /// Counts slumps from the valley walls, so a renderer can redraw the
    /// walls after bluff erosion.
    pub wall_version: u64,
    rng: Rng,
    /// Seed of the jitter's field, fixed at reset.
    jitter_seed: u64,
    /// Draws for the floodplain's events (falling wood, new mills, scroll
    /// bars), from a stream of their own so they leave the river's draws
    /// unchanged.
    events: Rng,
    /// Draws for the tributaries' courses, from a stream of their own.
    streams: Rng,
    /// Recent cut points and the step count until which they suppress new
    /// cutoffs nearby.
    recent_cuts: Vec<(P, u64)>,
    // Scratch buffers, kept to avoid per-step allocation.
    curv: Vec<f64>,
    rate: Vec<f64>,
    noise: Vec<f64>,
    tmp: Vec<P>,
    wtmp: Vec<f64>,
    grid_start: Vec<u32>,
    grid_items: Vec<u32>,
    grid_fill: Vec<u32>,
    cell_of: Vec<u32>,
    contacts: Vec<(Contact, Contact)>,
    /// Bank displacement of each point in the last migration, in world
    /// units along its right normal.
    shift: Vec<f64>,
    /// Cumulative weights for picking a point.
    weights: Vec<f64>,
    /// Bluff erosion found this step: the wall, x in channel widths, the
    /// retreat and its limit.
    cuts: Vec<(usize, f64, f64, f64)>,
    /// Whether each point may migrate (1) or a weir holds it (0), the
    /// width it relaxes to, and the factor on the overbank silt laid beside
    /// it; set by [`Sim::migration_rate`].
    held: Vec<f64>,
    target: Vec<f64>,
    boost: Vec<f64>,
    /// Valley floor's y range at each erodibility grid column, refreshed
    /// at the start of each survey of the land.
    valley: Vec<(f64, f64)>,
    /// Counts changes to the centreline within a step, so the grid built
    /// for one pass can serve the next if the centreline has not moved.
    line_version: u64,
    /// The grid's layout, and the `line_version` it was built for.
    grid_for: Option<(Cells, u64)>,
}

pub(crate) fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Valley width at `x`, `valley` (the whole domain height when 0),
/// narrowing smoothly to `pinch` at each of `pinches` over `length` either
/// side, and the fraction of the way from `pinch` to the full width it has
/// widened, which is 1 away from the pinches.
fn valley_at(
    x: f64,
    height: f64,
    valley: f64,
    pinches: &[f64],
    pinch: f64,
    length: f64,
) -> (f64, f64) {
    let full = if valley > 0.0 {
        valley.min(height)
    } else {
        height
    };
    if pinch <= 0.0 || pinch >= full || length <= 0.0 {
        return (full, 1.0);
    }
    let d = pinches
        .iter()
        .fold(f64::INFINITY, |d, &px| d.min((x - px).abs()));
    let f = smoothstep(d / length);
    (pinch + (full - pinch) * f, f)
}

/// Erodibility multiplier that falls smoothly from 1 to 0 within `margin` of
/// the domain edges and of the valley floor's edges `floor`.
fn edge_taper(p: P, bounds: P, margin: f64, floor: (f64, f64)) -> f64 {
    if margin <= 0.0 {
        return 1.0;
    }
    let ramp = |d: f64| smoothstep(d / margin);
    let y0 = floor.0.max(0.0);
    let y1 = floor.1.min(bounds[1]);
    ramp(p[0]) * ramp(bounds[0] - p[0]) * ramp(p[1] - y0) * ramp(y1 - p[1])
}

/// Hidden reaches the page runs upstream and downstream of its window, in
/// channel widths. See [`Sim::reset_view`].
pub const UPSTREAM: f64 = 60.0;
pub const DOWNSTREAM: f64 = 20.0;
/// Distance beyond each edge of [`Sim::window`], in channel widths, to
/// which the holdings reach, so the fields run on past the edge.
pub const WINDOW_MARGIN: f64 = 2.0;

/// Steps between [`Sim::check`]'s tests for the centreline crossing itself.
pub const CHECK_CROSSING: u64 = 10;

/// Mixed into the seed for the stream of floodplain events.
const EVENTS_SEED: u64 = 0x5eed_f10d;
/// Mixed into the seed for the stream of tributary course draws.
const STREAMS_SEED: u64 = 0x57e4_a3c1;
/// Mixed into the seed for the jitter's field.
const JITTER_SEED: u64 = 0x717e_b1e5;
/// Mixed into the jitter's seed with each redraw of its field.
const JITTER_DRAW: u64 = 0xD1B5_4A32_D192_ED03;
/// Mean, over a redraw of the jitter's field, of the push blended from two
/// draws and rescaled to unit RMS, for each unit of either draw: √2 asinh 1.
/// Dividing by it, and by the square root of `jitter_time`, gives the river
/// the same random drift over long times as a fresh push each step.
const JITTER_BLEND: f64 = 1.2465;

/// Feature size of the erodibility noise, in channel widths at reset.
const NOISE_SCALE: f64 = 25.0;

/// Cell size of the erodibility grid, in channel widths at reset.
const FIELD_CELL: f64 = 0.5;

/// Distance past the bank at which a point reads the erodibility of the
/// bank it moves into, in channel widths. The read misses the cells the
/// channel itself has just swept.
const BANK_BEYOND: f64 = 0.25;

/// Default width parameters; see [`Params`].
const WIDTH_TIME: f64 = 3.0;
const BAR_PUSH: f64 = 0.6;

/// Default slope feedback; see [`Params`].
const SLOPE_EXPONENT: f64 = 0.5;
const SLOPE_SINUOSITY: f64 = 1.25;

/// Arc length either side of a point over which its reach's sinuosity, and
/// so its slope, is measured, in channel widths: about one meander
/// wavelength, so a single bend does not set it alone.
pub const SLOPE_REACH: f64 = 12.0;

/// Range of the local width, in multiples of the reference width.
pub const MIN_WIDTH: f64 = 0.5;
pub const MAX_WIDTH: f64 = 2.0;

/// Laplacian smoothing of the local width per step, so it varies over a
/// few widths along the river and not from point to point.
const WIDTH_SMOOTHING: f64 = 0.25;

/// Default point-bar parameters; see [`Params`].
const BAR_FACTOR: f64 = 1.3;
const VEGETATION_TIME: f64 = 100.0;
/// Default forest succession; see [`Params`]. Riparian forest holds its
/// banks: Micheli et al. (2004) found the Sacramento River migrated about
/// 1.5 times as fast through cleared farmland as through it.
const SHRUB_FACTOR: f64 = 1.15;
const FOREST_TIME: f64 = 1500.0;
/// Default vegetation growth; see [`Params`].
const VEGETATION_HEIGHT: f64 = 0.05;
const SCROLL_HEIGHT: f64 = 0.05;
/// Steps between vegetation growth updates of a cell. Each step grows one
/// of this many bands of rows, so the cost is the same at every step.
const GROWTH_EVERY: u64 = 10;
/// Steps between the overbank deposits on a cell, laid a band of rows a
/// step as growth is: 5 units, 12.5 years, more often than the levees are
/// drawn.
const DEPOSIT_EVERY: u64 = 50;
/// Default landholdings; see [`Params`]. A medieval holding changed hands
/// about once a generation, some 30 years, but its hedges and headlands
/// mostly stayed put, and dated hedges show field boundaries lasting
/// centuries. A holding redraws its boundaries every 150 years on average
/// (60 units of 2.5 years). With a channel 40 m wide, 250 square widths is
/// 40 ha, about the hide of 120 acres that supported a household.
const HOLDING_AREA: f64 = 250.0;
const REORGANISE_TIME: f64 = 60.0;
/// Grass and willow take a fresh bar within a few years, and new alluvium
/// was grazed, then mown, within a generation or two of the river leaving
/// it: 20 units, 50 years.
const GRAZING_TIME: f64 = 20.0;
/// Under partible inheritance an heir often built beside the parent's
/// farmstead, so about half of all divisions make a hamlet.
const HAMLET_CHANCE: f64 = 0.5;

/// Default chute cutoffs; see [`Params::chute_rate`]. A chute runs
/// between two points of the channel from `CHUTE_CHORD.0` to
/// `CHUTE_CHORD.1` widths apart, across a bend at least `CHUTE_RATIO` times
/// as long as the chute and at most `CHUTE_ARC` widths long. The scan
/// covers `CHUTE_SCAN` points a step.
const CHUTE_RATE: f64 = 0.005;
const CHUTE_CHORD: (f64, f64) = (1.5, 5.0);
const CHUTE_RATIO: f64 = 3.0;
const CHUTE_ARC: f64 = 40.0;
const CHUTE_SCAN: usize = 8;

/// Steps during which a cut suppresses further cutoffs around it.
const CUT_COOLDOWN: u64 = 100;

/// Default overbank deposition, lake filling and scroll bars; see
/// [`Params`]. The rates are the means of the floods the model once drew,
/// one every 50 units (125 years) on average, of magnitude uniform in
/// [0, 1): a flood of magnitude 1 laid 0.3 channel depths at the bank and
/// filled a fifth of a lake near the channel with bedload, so the mean
/// rates are 0.3 and 0.2 times 0.5 / 50. Scroll bars keep the floods'
/// mean spacing of 125 years.
const CAPTURE_RATIO: f64 = 1.3;
const OVERBANK_RATE: f64 = 0.003;
const OXBOW_LIFETIME: f64 = 1200.0;
/// Time over which the fill brought into a lake settles, as the e-folding
/// time. At the default speed it is under two seconds on screen.
const SETTLE_TIME: f64 = 10.0;
const BEDLOAD_RATE: f64 = 0.002;
const SCROLL_TIME: f64 = 50.0;
const SCROLL_LIFETIME: f64 = 1000.0;

/// Distance of a scroll bar past the inner bank, in channel widths.
const SCROLL_OFFSET: f64 = 0.1;

/// Least migration rate, in the dimensionless units of
/// [`Sim::migration_rate`], at which a bank counts as accreting a scroll bar.
const SCROLL_MIN_RATE: f64 = 0.05;

/// Scroll bars and pieces with fewer points than this are dropped.
const MIN_SCROLL_POINTS: usize = 6;

/// Steps between checks for scroll bars under the channel. The ridges
/// they raise stay in the ground until the channel sweeps it, so dropping
/// a drawn scroll bar does not affect the dynamics and the check need not
/// run every step.
const SCROLL_REWORK_EVERY: u64 = 10;
const LEVEE_LENGTH: f64 = 2.0;
const LEVEE_HEIGHT: f64 = 0.25;

/// Default coarse ground; see [`Params`]. Gravel and colluvium erode
/// at 0.4 of the floodplain's rate.
const COARSE_FACTOR: f64 = 0.4;
/// Default upland clearance; see [`Params`]. It begins 100 units, 250
/// years, after the default warm-up of 500, and the supply triples over
/// the 1,000 years that follow, as the post-medieval alluvium of English
/// valleys shows sediment yields rising severalfold after clearance.
const CLEARANCE_START: f64 = 600.0;
const CLEARANCE_TIME: f64 = 400.0;
const CLEARANCE_SUPPLY: f64 = 3.0;
/// Overbank deposit, as a fraction of the flood level, above which a
/// scroll bar's ridge is buried and dropped.
const BURY: f64 = 0.95;
/// Default large wood; see [`Params`]. A jam of full size lasts 12 units,
/// 30 years, about the life of a jam in a temperate lowland river.
const WOOD_RATE: f64 = 0.3;
const JAM_LIFETIME: f64 = 12.0;
const JAM_PUSH: f64 = 0.15;
/// Wood a farmed bank gives, as a fraction of a forested one's: the
/// alders and willows left along the bank and the hedgerow trees.
pub const FARM_WOOD: f64 = 0.3;
/// Distance along the channel either side of a jam over which it pushes
/// the flow, in channel widths.
pub const JAM_REACH: f64 = 3.0;
/// Distance within which a new tree joins a jam already there, on either
/// bank, in channel widths.
const JAM_MERGE: f64 = 4.0;
/// Most jams at once.
const MAX_JAMS: usize = 40;
/// Default bluff erosion; see [`Params`]. Valley sides of terrace gravel
/// and bedrock erode a few times slower than the floodplain.
const BLUFF_RATE: f64 = 0.05;
/// Most a wall retreats, in channel widths, and the least room it leaves
/// between its foot and the domain edge.
const BLUFF_MAX: f64 = 8.0;
const BLUFF_ROOM: f64 = 1.0;
/// Colluvium a slump brings down: the radius of its lobe at the foot of
/// the wall, in channel widths, its height there in channel depths, and
/// its share of coarse material.
const SLUMP_RADIUS: f64 = 1.5;
const SLUMP_HEIGHT: f64 = 0.4;
const SLUMP_COARSE: f64 = 0.8;
/// Default tributary fans; see [`Params`]. A fan the channel trims
/// builds back over 400 units, 1,000 years, at the wooded upland's supply.
const FAN_RADIUS: f64 = 7.0;
const FAN_TIME: f64 = 400.0;
const TRIBUTARY_BOOST: f64 = 0.3;
/// The deepest combe's tributary fills the head of its delta in 100 units,
/// 250 years, and each tributary moves its course on its fan every 400
/// units, 1,000 years, on average.
const DELTA_RATE: f64 = 0.01;
const STREAM_SHIFT_RATE: f64 = 0.0025;
/// Combe depth, in channel widths, below which a combe carries no
/// tributary, and the depth of the deepest combe; see [`walls`].
pub const FAN_MIN_DEPTH: f64 = 3.0;
pub const FAN_MAX_DEPTH: f64 = 4.5;
/// Height of a fan at its apex above fresh bar sand, in channel depths.
const FAN_HEIGHT: f64 = 0.6;
/// Distance downstream of a confluence over which the tributary's bedload
/// speeds migration, in channel widths.
pub const TRIBUTARY_REACH: f64 = 30.0;
/// Steps between the walks along each tributary's course for the water
/// it meets first.
const JUNCTION_EVERY: u64 = 10;
/// Default mills; see [`Params`]. The Domesday survey of 1086 counted
/// about 5,600 mills in England, several to a river every few kilometres;
/// with a channel 40 m wide, 50 widths is 2 km.
const MILL_RATE: f64 = 0.05;
const MILL_GAP: f64 = 50.0;
/// A weir's pond lowers the slope of the reach above it; at 0.5 the mills
/// held reaches straight enough to lower the river's mean sinuosity by
/// 0.12, and at 0.25 by 0.06.
const WEIR_BACKWATER: f64 = 0.25;
const POND_LENGTH: f64 = 15.0;
/// Distance along the channel either side of a weir over which its
/// masonry holds the banks, in channel widths.
const WEIR_HOLD: f64 = 2.0;
/// Width a mill pond relaxes to just above its weir, in multiples of the
/// reference width, and the factor on overbank silt there, each falling
/// to 1 at `pond_length`.
const POND_WIDEN: f64 = 0.4;
const POND_SILT: f64 = 1.0;
/// Distance within which the channel must pass a weir to keep working it,
/// in channel widths.
const MILL_SNAP: f64 = 0.75;
/// Cosine of the turn over a channel width near a weir past which the
/// channel has outflanked it: 45°.
const OUTFLANK_COS: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Least ratio of chord to arc along the reach a mill's leat runs beside.
const MILL_STRAIGHT: f64 = 0.95;
/// Leat: it leaves the channel `LEAT_UP` widths above the weir and returns
/// `LEAT_DOWN` below it, running `LEAT_OFFSET` widths beyond the bank.
const LEAT_UP: f64 = 7.0;
const LEAT_DOWN: f64 = 3.0;
const LEAT_OFFSET: f64 = 1.5;
/// Time an abandoned mill stands as a ruin before it is dropped.
pub const MILL_RUIN: f64 = 200.0;

/// Time the parish boundary is set on the channel's course: 1,000 years,
/// as English parish boundaries were fixed in the tenth to twelfth
/// centuries and kept where the rivers they followed then ran.
pub const BOUNDARY_TIME: f64 = 400.0;

/// Oxbow pieces with fewer points than this are dropped.
const MIN_OXBOW_POINTS: usize = 3;

/// Distance from the channel centreline, in channel widths, within which
/// the channel removes an oxbow point. The lakes lie under the water, so a
/// point nearer than half a width is already hidden on the channel side; a
/// smaller radius lets a channel that runs alongside a lake cover it a
/// little at a time, not a whole reach at once. It is above half the
/// largest step, `max_step` × `SPACING` = 0.075, so a channel moving across
/// a point comes within it.
const REWORK_RADIUS: f64 = 0.1;

/// Fill above which an oxbow lake no longer captures the channel: the clay
/// plug has closed the route through it.
pub const CAPTURE_FILL: f64 = 0.5;

/// Length of the plug that seals each end of an oxbow soon after the cut,
/// in channel widths. Bedload settles where flow into the old bend slows,
/// so the ends close before the rest of the lake fills. The channel meeting
/// a lake there opens no route through it.
const END_PLUG: f64 = 3.0;

/// Layout of the hashed grid of centreline points: the cell size, the
/// bucket mask, and the step in buckets from one row of cells to the next.
#[derive(Clone, Copy)]
struct Cells {
    size: f64,
    mask: usize,
    stride: i64,
}

impl Cells {
    fn cell(&self, p: P) -> (i64, i64) {
        (
            (p[0] / self.size).floor() as i64,
            (p[1] / self.size).floor() as i64,
        )
    }

    /// Cells along a row hash to consecutive buckets, so the three cells
    /// around a point in a row are one run of the table.
    fn bucket(&self, cx: i64, cy: i64) -> usize {
        cy.wrapping_mul(self.stride).wrapping_add(cx) as usize & self.mask
    }
}

/// Where a lake's bank meets the channel's: the oxbow point, the nearest
/// centreline point whose bank meets it, and the squared distance between
/// them.
type Contact = (usize, usize, f64);

/// Whether segments `ab` and `cd` cross at a point interior to both.
fn segments_cross(a: P, b: P, c: P, d: P) -> bool {
    fn orient(a: P, b: P, c: P) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    orient(a, b, c) * orient(a, b, d) < 0.0 && orient(c, d, a) * orient(c, d, b) < 0.0
}

impl Sim {
    pub fn new(params: Params, bounds: P, seed: u64, noise: f64) -> Self {
        let mut sim = Self {
            params,
            bounds,
            pinches: Vec::new(),
            window: [f64::NEG_INFINITY, f64::INFINITY],
            walls: Walls::default(),
            centerline: Vec::new(),
            widths: Vec::new(),
            slope: Vec::new(),
            oxbows: Vec::new(),
            oxbow_version: 0,
            scrolls: Vec::new(),
            scroll_version: 0,
            erodibility: Erodibility::new([1.0, 1.0], 1.0, 1.0, 0),
            land: Land::new(&Erodibility::new([1.0, 1.0], 1.0, 1.0, 0), 0),
            time: 0.0,
            steps: 0,
            cutoffs: 0,
            captures: 0,
            chutes: 0,
            chute_at: 0,
            jams: Vec::new(),
            tributaries: Vec::new(),
            mills: Vec::new(),
            boundary: Vec::new(),
            tracks: Tracks::default(),
            mill_version: 0,
            land_mills: u64::MAX,
            land_streams: u64::MAX,
            wall_version: 0,
            rng: Rng::new(seed),
            jitter_seed: seed ^ JITTER_SEED,
            events: Rng::new(seed ^ EVENTS_SEED),
            streams: Rng::new(seed ^ STREAMS_SEED),
            recent_cuts: Vec::new(),
            curv: Vec::new(),
            rate: Vec::new(),
            noise: Vec::new(),
            tmp: Vec::new(),
            wtmp: Vec::new(),
            grid_start: Vec::new(),
            grid_items: Vec::new(),
            grid_fill: Vec::new(),
            cell_of: Vec::new(),
            contacts: Vec::new(),
            shift: Vec::new(),
            weights: Vec::new(),
            cuts: Vec::new(),
            held: Vec::new(),
            target: Vec::new(),
            boost: Vec::new(),
            valley: Vec::new(),
            line_version: 0,
            grid_for: None,
        };
        sim.reset(seed, noise);
        sim
    }

    /// Restart as the page runs the river, for a visible window `view_w` by
    /// `view_h` world units starting at `UPSTREAM` widths. Hidden reaches
    /// let meanders develop before they come into view and keep the pinned
    /// outlet out of sight, and the valley is pinched at the window edges so
    /// no bend straddles them. Returns the window's left edge.
    pub fn reset_view(&mut self, view_w: f64, view_h: f64, seed: u64, noise: f64) -> f64 {
        let w = self.params.width;
        let x = UPSTREAM * w;
        self.bounds = [(UPSTREAM + DOWNSTREAM) * w + view_w, view_h];
        self.pinches = vec![x, x + view_w];
        self.window = [x, x + view_w];
        self.reset(seed, noise);
        x
    }

    /// Restart from a straight line across the domain with lateral noise of
    /// amplitude `noise` channel widths.
    pub fn reset(&mut self, seed: u64, noise: f64) {
        self.rng = Rng::new(seed);
        self.jitter_seed = seed ^ JITTER_SEED;
        self.events = Rng::new(seed ^ EVENTS_SEED);
        self.streams = Rng::new(seed ^ STREAMS_SEED);
        let [w, h] = self.bounds;
        let line = vec![[0.0, h / 2.0], [w, h / 2.0]];
        let mut pts = resample(&line, self.ds());
        let amp = noise * self.params.width;
        let n = pts.len();
        // White noise, then a few smoothing passes so the perturbation has
        // energy at meander scales and not only at the point spacing.
        let mut off: Vec<f64> = (0..n).map(|_| self.rng.normal() * amp).collect();
        for _ in 0..8 {
            let prev = off.clone();
            for i in 1..n - 1 {
                off[i] = 0.25 * prev[i - 1] + 0.5 * prev[i] + 0.25 * prev[i + 1];
            }
        }
        off[0] = 0.0;
        off[n - 1] = 0.0;
        for (p, o) in pts.iter_mut().zip(&off) {
            p[1] += o;
        }
        self.widths = vec![1.0; pts.len()];
        self.centerline = pts;
        let w = self.params.width;
        self.erodibility = Erodibility::new(self.bounds, FIELD_CELL * w, NOISE_SCALE * w, seed);
        self.walls = Walls::new(self.bounds[0] / w, seed);
        self.oxbows.clear();
        self.oxbow_version += 1;
        self.line_version += 1;
        self.scrolls.clear();
        self.scroll_version += 1;
        self.recent_cuts.clear();
        self.time = 0.0;
        self.steps = 0;
        self.cutoffs = 0;
        self.captures = 0;
        (self.chutes, self.chute_at) = (0, 0);
        self.jams.clear();
        self.mills.clear();
        self.boundary.clear();
        self.tracks.clear();
        self.mill_version += 1;
        self.wall_version += 1;
        self.place_tributaries();
        self.land = Land::new(&self.erodibility, seed);
        let margin = WINDOW_MARGIN * w;
        self.land
            .confine(self.window[0] - margin, self.window[1] + margin, w);
        self.land_streams = u64::MAX;
        self.read_valley();
        let s = self.land_settings();
        self.land.update(
            &self.erodibility,
            &self.centerline,
            &self.widths,
            &self.valley,
            &s,
        );
    }

    /// The valley floor's y range at `x`, between the feet of the walls,
    /// narrowed at the pinches if `pinched`. The pinches keep bends off the
    /// window edges and are not drawn, so the land reads the floor without
    /// them. The walls' wander fades out at a pinch with the valley's width.
    pub fn floor(&self, x: f64, pinched: bool) -> (f64, f64) {
        let (p, h) = (&self.params, self.bounds[1]);
        if p.valley_width <= 0.0 {
            return (0.0, h);
        }
        let (width, f) = if pinched {
            valley_at(
                x,
                h,
                p.valley_width * p.width,
                &self.pinches,
                p.pinch_width * p.width,
                p.pinch_length * p.width,
            )
        } else {
            ((p.valley_width * p.width).min(h), 1.0)
        };
        let (mid, xw) = (0.5 * h, x / p.width);
        let out = |side| f * p.width * self.walls.out(side, xw);
        (mid - 0.5 * width - out(0), mid + 0.5 * width + out(1))
    }

    /// Refresh the valley floor's y range at each erodibility grid column.
    fn read_valley(&mut self) {
        let cell = self.erodibility.cell;
        let mut valley = std::mem::take(&mut self.valley);
        valley.clear();
        valley
            .extend((0..self.erodibility.cols).map(|x| self.floor((x as f64 + 0.5) * cell, false)));
        self.valley = valley;
    }

    fn land_settings(&self) -> land::Settings {
        let p = &self.params;
        land::Settings {
            time: self.time,
            width: p.width,
            holding_area: p.holding_area,
            reorganise_time: p.reorganise_time,
            grazing_time: p.grazing_time,
            levee_height: p.levee_height,
            farmsteads: p.farmsteads,
            hamlet_chance: p.hamlet_chance,
        }
    }

    /// Target point spacing Δs, in world units.
    pub fn ds(&self) -> f64 {
        SPACING * self.params.width
    }

    /// Check the invariants the model keeps, for the seed fuzzer: a finite
    /// centreline inside the domain that does not cross itself, local
    /// widths within their bounds, whole oxbows, and, right after a
    /// survey's weathering, no building across a stream. The crossing test
    /// is quadratic in the points, so it runs every [`CHECK_CROSSING`]
    /// steps.
    pub fn check(&self) -> Result<(), String> {
        let [w, h] = self.bounds;
        let pts = &self.centerline;
        if pts.len() < 2 {
            return Err(format!("centreline has {} points", pts.len()));
        }
        let outside = |p: &P| !(p[0] >= 0.0 && p[0] <= w && p[1] >= 0.0 && p[1] <= h);
        if let Some(i) = pts.iter().position(outside) {
            return Err(format!(
                "centreline point {i} at {:?} is outside the domain",
                pts[i]
            ));
        }
        if self.widths.len() != pts.len() {
            return Err(format!(
                "{} widths for {} points",
                self.widths.len(),
                pts.len()
            ));
        }
        if let Some(i) = self
            .widths
            .iter()
            .position(|b| !(MIN_WIDTH..=MAX_WIDTH).contains(b))
        {
            return Err(format!("local width {} at point {i}", self.widths[i]));
        }
        for (k, o) in self.oxbows.iter().enumerate() {
            let whole = o.points.len() == o.widths.len()
                && !o.points.iter().any(outside)
                && (0.0..=1.0).contains(&o.fill);
            if !whole {
                return Err(format!("oxbow {k} is malformed"));
            }
        }
        if self.steps.is_multiple_of(CHECK_CROSSING) && geom::self_intersects(pts) {
            return Err("centreline crosses itself".into());
        }
        if self.steps % land::SURVEY_STEPS == 2 * land::BANDS as u64 {
            let across = (self.land.buildings.iter())
                .filter(|b| self.land.across_stream(b, self.params.width))
                .count();
            if across > 0 {
                return Err(format!("{across} buildings across a stream"));
            }
        }
        Ok(())
    }

    /// Arc length over straight-line distance between the endpoints.
    pub fn sinuosity(&self) -> f64 {
        let pts = &self.centerline;
        let chord = dist(pts[0], pts[pts.len() - 1]);
        if chord > 0.0 {
            geom::length(pts) / chord
        } else {
            1.0
        }
    }

    pub fn step_n(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    pub fn step(&mut self) {
        let ds = self.ds();
        self.resample(ds);
        self.track();
        self.migrate();
        self.erode_bluffs();
        self.smooth();
        self.pin_weirs();
        self.clamp_to_domain();
        self.line_version += 1;
        if self.cutoff() {
            self.resample(ds);
            self.track();
        }
        if self.capture() {
            self.resample(ds);
            self.track();
        }
        if self.chute() {
            self.resample(ds);
            self.track();
        }
        // Scroll bars come with high water, as a Poisson process,
        // approximated per step.
        let p = &self.params;
        if p.scroll_time > 0.0 && self.events.uniform() < p.dt / p.scroll_time {
            self.add_scrolls();
        }
        self.shift_streams();
        self.erodibility.sweep(&self.centerline);
        self.time += self.params.dt;
        self.steps += 1;
        let (p, rows) = (&self.params, self.erodibility.rows as u64);
        let band = self.steps % GROWTH_EVERY;
        let (y0, y1) = (band * rows / GROWTH_EVERY, (band + 1) * rows / GROWTH_EVERY);
        self.erodibility.grow_rows(
            GROWTH_EVERY as f64 * p.dt,
            p.vegetation_height,
            y0 as usize,
            y1 as usize,
        );
        let band = self.steps % DEPOSIT_EVERY;
        let (y0, y1) = (
            band * rows / DEPOSIT_EVERY,
            (band + 1) * rows / DEPOSIT_EVERY,
        );
        self.lay_overbank(y0 as usize, y1 as usize);
        if band == 0 {
            self.fill_lakes(DEPOSIT_EVERY as f64 * self.params.dt);
        }
        self.feed_tributaries();
        if self.boundary.is_empty() && self.time >= BOUNDARY_TIME {
            self.boundary = self.centerline.clone();
            self.land.set_boundary(&self.boundary, self.bounds[0]);
        }
        let phase = self.steps % land::SURVEY_STEPS;
        if phase == 0 {
            self.read_valley();
            let filled = self.oxbows.iter().filter(|o| o.filled_at.is_finite());
            self.land.set_scars(filled.map(|o| &o.points[..]));
        }
        let streams = self.tributaries.iter().map(|t| t.version + 1).sum();
        if self.land_streams != streams {
            self.land_streams = streams;
            let courses = self.tributaries.iter().map(|t| (&t.course[..], t.scale));
            self.land.set_streams(courses, self.params.width);
        }
        if self.land_mills != self.mill_version {
            self.land_mills = self.mill_version;
            self.land.mills = (self.mills.iter().filter(|m| m.working()))
                .filter_map(|m| m.leat.get(m.leat.len() / 2).copied())
                .collect();
        }
        let s = self.land_settings();
        self.land.tick(
            &self.erodibility,
            &self.centerline,
            &self.widths,
            &self.valley,
            &s,
            phase,
        );
        (self.tracks).tick(
            &self.land,
            &self.erodibility,
            self.params.width,
            self.time,
            phase == 0,
        );
        self.recruit_wood();
        self.found_mill();
        self.rework_oxbows();
        // Clearance fills lakes faster with the fine sediment it washes in.
        let supply = self.supply();
        let (time, rate) = (
            self.time,
            supply * self.params.dt / self.params.oxbow_lifetime.max(1e-9),
        );
        // A lake fills at least as far as the silt on its bed, read every
        // step, as the overbank silt raises the bed and the channel sweeps
        // some of its cells.
        let settle = (self.params.dt / SETTLE_TIME).min(1.0);
        let field = &self.erodibility;
        for o in self.oxbows.iter_mut().filter(|o| o.is_lake()) {
            o.settling = o.settling.max(field.lake_silt(&o.points).min(1.0) - o.fill);
            let d = settle * o.settling;
            o.settling -= d;
            o.fill = (o.fill + rate + d).min(1.0);
            if o.fill >= 1.0 {
                o.filled_at = time;
            }
        }
        let (scar, before) = (self.params.scar_lifetime, self.oxbows.len());
        self.oxbows.retain(|o| time - o.filled_at < scar);
        if self.oxbows.len() != before {
            self.oxbow_version += 1;
        }
        if self.steps.is_multiple_of(SCROLL_REWORK_EVERY) {
            self.rework_scrolls();
        }
        let steps = self.steps;
        self.recent_cuts.retain(|&(_, until)| until > steps);
    }

    /// Sediment supply from the upland, as a multiple of the wooded
    /// upland's: 1 until `clearance_start`, rising smoothly to
    /// `clearance_supply` over `clearance_time`.
    pub fn supply(&self) -> f64 {
        let p = &self.params;
        let t = if p.clearance_time > 0.0 {
            (self.time - p.clearance_start) / p.clearance_time
        } else if self.time >= p.clearance_start {
            1.0
        } else {
            0.0
        };
        1.0 + (p.clearance_supply - 1.0) * smoothstep(t)
    }

    /// Lay the overbank silt of [`DEPOSIT_EVERY`] steps over the rows from
    /// `y0` up to `y1`, one band of the grid, so each cell gains it once a
    /// cycle of bands. Clearance raises the silt the river carries. How far
    /// it spreads from the bank is set by how fast it settles, which
    /// clearance leaves alone.
    fn lay_overbank(&mut self, y0: usize, y1: usize) {
        let supply = self.supply();
        let p = &self.params;
        // A cutoff this step leaves the factors of the old points, which
        // the next step's migration replaces.
        let boost: &[f64] = if self.boost.len() == self.centerline.len() {
            &self.boost
        } else {
            &[]
        };
        let f = Overbank {
            half: 0.5 * p.width,
            amount: DEPOSIT_EVERY as f64 * p.dt * p.overbank_rate * supply,
            length: p.levee_length * p.width,
            stage: p.levee_height,
            boost,
        };
        self.erodibility
            .lay_overbank(&self.centerline, &self.widths, &f, y0, y1);
    }

    /// Bring the jams and mills up to date with the centreline's points.
    fn track(&mut self) {
        self.track_jams();
        self.track_mills();
    }

    /// Index of the centreline point nearest `p` within `r`, at most the
    /// cell size of `g`.
    fn nearest_point(&self, p: P, r: f64, g: Cells) -> Option<usize> {
        let (cx, cy) = g.cell(p);
        let mut best = (None, r * r);
        for y in cy - 1..=cy + 1 {
            for part in self.row(g, cx, y) {
                for &j in part {
                    let d = geom::dist2(p, self.centerline[j as usize]);
                    if d < best.1 {
                        best = (Some(j as usize), d);
                    }
                }
            }
        }
        best.0
    }

    /// Erode the valley walls where banks press against them. A bank
    /// moving towards a wall cuts it back by `bluff_rate` times its
    /// migration demand, times the square of the shortfall from 1 of the
    /// wall's taper at the bank, spread along the wall as a scallop. The
    /// river then reaches farther into the wall as the taper follows it.
    /// The walls' feet are read without the pinches, which keep the river
    /// off the walls there anyway. Each slump drops a lobe of colluvium at
    /// the foot.
    fn erode_bluffs(&mut self) {
        let p = &self.params;
        let (w, n) = (p.width, self.centerline.len());
        if p.bluff_rate <= 0.0
            || p.valley_width <= 0.0
            || p.edge_margin <= 0.0
            || self.rate.len() != n
        {
            return;
        }
        let k = p.bluff_rate * p.dt * p.erodibility;
        let (h, margin) = (self.bounds[1], p.edge_margin * w);
        self.cuts.clear();
        for i in 1..n.saturating_sub(1) {
            let r = self.rate[i];
            let q = self.centerline[i];
            let nrm = right_normal(&self.centerline, i);
            let toward = r.signum() * nrm[1];
            if toward == 0.0 {
                continue;
            }
            let off = r.signum() * (0.5 * self.widths[i] + BANK_BEYOND) * w;
            let bank = q[1] + off * nrm[1];
            let floor = self.floor(q[0], false);
            let (side, gap, room) = if toward < 0.0 {
                (0, bank - floor.0, floor.0)
            } else {
                (1, floor.1 - bank, h - floor.1)
            };
            let short = 1.0 - smoothstep(gap / margin);
            if short <= 0.0 {
                continue;
            }
            let xw = q[0] / w;
            let limit = (self.walls.retreat(side, xw) + room / w - BLUFF_ROOM).min(BLUFF_MAX);
            self.cuts
                .push((side, xw, k * r.abs() * toward.abs() * short * short, limit));
        }
        for c in 0..self.cuts.len() {
            let (side, xw, amount, limit) = self.cuts[c];
            if let Some(x) = self.walls.erode(side, xw, amount, limit) {
                let floor = self.floor(x * w, false);
                let foot = [x * w, if side == 0 { floor.0 } else { floor.1 }];
                self.erodibility
                    .grow_cone(foot, SLUMP_RADIUS * w, SLUMP_HEIGHT, SLUMP_COARSE, 1.0);
                self.wall_version += 1;
            }
        }
    }

    /// Resample the centreline to spacing close to `ds`.
    fn resample(&mut self, ds: f64) {
        geom::resample_with(
            &self.centerline,
            &self.widths,
            ds,
            &mut self.tmp,
            &mut self.wtmp,
        );
        std::mem::swap(&mut self.centerline, &mut self.tmp);
        std::mem::swap(&mut self.widths, &mut self.wtmp);
        self.line_version += 1;
    }

    /// Erodibility the ground lends a bank, from the parameters and the
    /// current time.
    fn ground(&self) -> Ground {
        let p = &self.params;
        Ground {
            time: self.time,
            heterogeneity: p.heterogeneity,
            lake_life: p.oxbow_lifetime,
            lake: p.lake_factor,
            clay: p.clay_factor,
            bar: p.bar_factor,
            vegetation: p.vegetation_time,
            shrub: p.shrub_factor,
            forest: p.forest_time,
            coarse: p.coarse_factor,
        }
    }

    /// Erodibility of the bank of point `i` on the side `sign` of its right
    /// normal `nrm`, read [`BANK_BEYOND`] past the bank and tapered at the
    /// domain edges and valley walls. The taper is read at the bank too, so
    /// a reach beside a wall can still move away from it.
    fn bank_erodibility(&self, i: usize, nrm: P, sign: f64, ground: &Ground) -> f64 {
        let p = &self.params;
        let q = self.centerline[i];
        let floor = self.floor(q[0], true);
        let side = sign * (0.5 * self.widths[i] + BANK_BEYOND) * p.width;
        let bank = [q[0] + side * nrm[0], q[1] + side * nrm[1]];
        edge_taper(bank, self.bounds, p.edge_margin * p.width, floor)
            * self.erodibility.at(bank, ground)
    }

    /// Howard–Knutson migration rate at each point, in dimensionless units
    /// (curvature measured in inverse channel widths).
    pub fn migration_rate(&mut self) -> &[f64] {
        let p = &self.params;
        let pts = &self.centerline;
        let n = pts.len();
        curvature(pts, &mut self.curv);
        // Hickin–Nanson saturation: migration grows with w/r for gentle
        // bends, peaks at r = peak_radius·w, and falls for tighter bends.
        // c / (1 + (c/c0)²) peaks at c0 = 1/peak_radius.
        let c0 = if p.peak_radius > 0.0 {
            1.0 / p.peak_radius
        } else {
            f64::INFINITY
        };
        for c in &mut self.curv {
            let cw = *c * p.width;
            *c = cw / (1.0 + (cw / c0).powi(2));
        }
        // Actual spacing after resampling, in channel widths.
        let ds_w = if n > 1 {
            geom::length(pts) / (n - 1) as f64 / p.width
        } else {
            SPACING
        };
        let decay = (-p.alpha * ds_w).exp();
        self.rate.clear();
        self.rate.resize(n, 0.0);
        let (mut acc, mut norm) = (0.0, 0.0);
        for i in 0..n {
            if i > 0 {
                acc = decay * (acc + self.curv[i - 1] * ds_w);
                norm = decay * (norm + ds_w);
            }
            let upstream = if norm > 0.0 { acc / norm } else { 0.0 };
            // The local term straightens the river like diffusion, and at
            // the time step a zigzag from point to point overshoots on soft
            // ground and flips each step. A [1, 6, 1] filter halves the
            // term on that zigzag, which still straightens kinks, and takes
            // 1.2% of it on a bend 10 widths long.
            let (a, b) = (
                self.curv[i.saturating_sub(1)],
                self.curv[(i + 1).min(n - 1)],
            );
            let local = 0.125 * (a + b) + 0.75 * self.curv[i];
            self.rate[i] = p.omega * local + p.gamma * upstream;
        }
        // The water surface falls as far as the valley floor over a reach,
        // so a reach of sinuosity σ has 1/σ of the valley slope. Points are
        // evenly spaced after resampling, so the reach's arc length is its
        // count of segments times the spacing.
        let k = ((SLOPE_REACH / ds_w).round() as usize).max(1);
        let spacing = ds_w * p.width;
        self.slope.clear();
        self.slope.extend((0..n).map(|i| {
            let (a, b) = (i.saturating_sub(k), (i + k).min(n - 1));
            let arc = (b - a) as f64 * spacing;
            if arc > 0.0 {
                (dist(pts[a], pts[b]) / arc).clamp(1e-3, 1.0)
            } else {
                1.0
            }
        }));
        // `powf` is a software routine in WASM, so the default exponent of
        // one half takes the square root, an instruction.
        let e = p.slope_exponent;
        if e != 0.0 {
            for (r, s) in self.rate.iter_mut().zip(&self.slope) {
                let x = p.slope_sinuosity * s;
                *r *= if e == 0.5 { x.sqrt() } else { x.powf(e) };
            }
        }
        self.feed_bars();
        self.push_jams();
        self.pond_weirs();
        &self.rate
    }

    fn migrate(&mut self) {
        self.migration_rate();
        let p = &self.params;
        let pts = &self.centerline;
        let n = pts.len();
        let scale = p.dt * p.erodibility * p.width;
        let limit = p.max_step * SPACING * p.width;
        let ground = self.ground();
        // Width lost per step to bar push towards the reference width.
        let relax = if p.width_time > 0.0 {
            (p.dt / p.width_time).min(1.0)
        } else {
            1.0
        };

        // Value noise over the floodplain on a lattice `2 jitter_scale`
        // widths apart, redrawn every `jitter_time` and blended between
        // draws, so the push at each point turns over that time. Rescaled to
        // the target RMS. Short-correlated noise only adds kinks.
        let tau = p.jitter_time.max(p.dt);
        let amp = p.jitter * p.width * p.dt / (JITTER_BLEND * tau.sqrt());
        let cell = 1.0 / (2.0 * p.jitter_scale.max(SPACING) * p.width);
        let draw = (self.time / tau).floor();
        let s = self.time / tau - draw;
        let seeds =
            [draw as u64, draw as u64 + 1].map(|k| self.jitter_seed ^ k.wrapping_mul(JITTER_DRAW));
        self.noise.clear();
        self.noise.extend(pts.iter().map(|q| {
            let (x, y) = (q[0] * cell, q[1] * cell);
            let [a, b] = seeds.map(|seed| value_noise(seed, x, y) - 0.5);
            a + (b - a) * s
        }));
        let rms = (self.noise.iter().map(|x| x * x).sum::<f64>() / n.max(1) as f64).sqrt();
        if rms > 0.0 {
            for x in &mut self.noise {
                *x *= amp / rms;
            }
        }

        self.tmp.clear();
        self.tmp.extend_from_slice(pts);
        self.wtmp.clear();
        self.wtmp.resize(n, 1.0);
        self.shift.clear();
        self.shift.resize(n, 0.0);
        for i in 1..n.saturating_sub(1) {
            let nrm = right_normal(pts, i);
            // Positive rate moves toward the outside of a left bend, i.e. to
            // the right, so bends grow.
            // The bank is on the side the rate pushes towards. Choosing it
            // by the jitter as well would damp outward and amplify inward
            // noise wherever the banks differ, a net drift inwards.
            let sign = if self.rate[i] < 0.0 { -1.0 } else { 1.0 };
            let e = self.bank_erodibility(i, nrm, sign, &ground);
            let held = self.held[i];
            let d = held * (e * (scale * self.rate[i] + self.noise[i])).clamp(-limit, limit);
            self.tmp[i][0] += d * nrm[0];
            self.tmp[i][1] += d * nrm[1];
            self.shift[i] = d;
            // The outer bank retreats as the centreline does, without the
            // jitter, and the inner bank follows at the bar push. The
            // centreline keeps the Howard–Knutson rate, so the two agree on
            // average.
            let (b, target) = (self.widths[i], self.target[i]);
            let demand = held * scale * self.rate[i].abs();
            let (pull, push) = ((e * demand).min(limit), (p.bar_push * demand).min(limit));
            self.wtmp[i] = if p.width_time > 0.0 {
                b + (pull - push) / p.width - (b - target) * relax
            } else {
                1.0
            };
        }
        std::mem::swap(&mut self.centerline, &mut self.tmp);
        // Smooth the width along the river, with the ends held at 1.
        for i in 1..n.saturating_sub(1) {
            let lap = self.wtmp[i - 1] + self.wtmp[i + 1] - 2.0 * self.wtmp[i];
            self.widths[i] = (self.wtmp[i] + WIDTH_SMOOTHING * lap).clamp(MIN_WIDTH, MAX_WIDTH);
        }
        if n > 0 {
            (self.widths[0], self.widths[n - 1]) = (1.0, 1.0);
        }
    }

    fn smooth(&mut self) {
        let k = self.params.smoothing;
        if k <= 0.0 {
            return;
        }
        let n = self.centerline.len();
        self.tmp.clear();
        self.tmp.extend_from_slice(&self.centerline);
        let src = &self.tmp;
        // A weir holds its points against smoothing as well as migration.
        for i in 1..n.saturating_sub(1) {
            let free = self.held.get(i).copied().unwrap_or(1.0);
            let (a, b, c) = (src[i - 1], src[i], src[i + 1]);
            // A point where the line turns back by more than 120 degrees
            // is a hairpin too short for a neck cutoff, left by a sharp
            // change of rate from one point to the next. It goes to the
            // middle of its neighbours, before the arms cross.
            let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
            let dot = u[0] * v[0] + u[1] * v[1];
            let uv = (u[0] * u[0] + u[1] * u[1]) * (v[0] * v[0] + v[1] * v[1]);
            let k = if dot < 0.0 && dot * dot > 0.25 * uv {
                0.5
            } else {
                k
            };
            let p = &mut self.centerline[i];
            p[0] += free * k * (a[0] + c[0] - 2.0 * b[0]);
            p[1] += free * k * (a[1] + c[1] - 2.0 * b[1]);
        }
    }

    fn clamp_to_domain(&mut self) {
        let [w, h] = self.bounds;
        for p in &mut self.centerline {
            p[0] = p[0].clamp(0.0, w);
            p[1] = p[1].clamp(0.0, h);
        }
    }

    /// Fill the hashed grid with the centreline points, for cells of size
    /// `size`, unless it already holds them in cells up to twice that size,
    /// which serve searches within `size` as well. The table has about 4n buckets
    /// and is filled by counting sort, so each bucket lists its points in
    /// increasing order; collisions only add candidates, which callers
    /// reject by distance.
    fn build_grid(&mut self, size: f64) -> Cells {
        self.build_grid_of(size, size)
    }

    /// As [`Self::build_grid`], but cells of `cell`, from `size` to twice
    /// it, if the grid must be filled again, so it also serves a later
    /// search within `cell`.
    fn build_grid_of(&mut self, size: f64, cell: f64) -> Cells {
        if let Some((g, v)) = self.grid_for
            && g.size >= size
            && g.size <= 2.0 * size
            && v == self.line_version
        {
            return g;
        }
        let size = cell.clamp(size, 2.0 * size);
        let pts = &self.centerline;
        let n = pts.len();
        let buckets = (4 * n).next_power_of_two();
        let mask = buckets - 1;
        let g = Cells {
            size,
            mask,
            stride: (self.bounds[0] / size).ceil() as i64 + 3,
        };
        self.grid_start.clear();
        self.grid_start.resize(buckets + 1, 0);
        self.cell_of.clear();
        for &p in pts {
            let (cx, cy) = g.cell(p);
            let b = g.bucket(cx, cy);
            self.cell_of.push(b as u32);
            self.grid_start[b + 1] += 1;
        }
        for b in 0..buckets {
            self.grid_start[b + 1] += self.grid_start[b];
        }
        self.grid_items.clear();
        self.grid_items.resize(n, 0);
        self.grid_fill.clear();
        self.grid_fill
            .extend_from_slice(&self.grid_start[..buckets]);
        for (i, &b) in self.cell_of.iter().enumerate() {
            let slot = &mut self.grid_fill[b as usize];
            self.grid_items[*slot as usize] = i as u32;
            *slot += 1;
        }
        self.grid_for = Some((g, self.line_version));
        g
    }

    /// Point indices in cells `cx − 1` to `cx + 1` of row `cy`, as one run
    /// of buckets, split in two where it wraps round the table.
    fn row(&self, g: Cells, cx: i64, cy: i64) -> [&[u32]; 2] {
        let (b, buckets) = (g.bucket(cx - 1, cy), g.mask + 1);
        let items = |a: usize, e: usize| {
            &self.grid_items[self.grid_start[a] as usize..self.grid_start[e] as usize]
        };
        if b + 3 <= buckets {
            [items(b, b + 3), &[]]
        } else {
            [items(b, buckets), items(0, b + 3 - buckets)]
        }
    }

    /// Call `f` with every point index above `after` in the 3 × 3 block of
    /// grid cells around `p`, which covers all points within the cell size
    /// of it, row by row and cell by cell, in increasing order in each.
    fn for_neighbours_after(&self, p: P, g: Cells, after: usize, mut f: impl FnMut(usize)) {
        let (cx, cy) = g.cell(p);
        for y in cy - 1..=cy + 1 {
            for part in self.row(g, cx, y) {
                for &j in part {
                    if j as usize > after {
                        f(j as usize);
                    }
                }
            }
        }
    }

    /// Whether any centreline point lies within `r`, at most the cell size,
    /// of `p`.
    fn near_channel(&self, p: P, r: f64, g: Cells) -> bool {
        let (cx, cy) = g.cell(p);
        let pts = &self.centerline;
        (cy - 1..=cy + 1).any(|y| {
            self.row(g, cx, y).iter().any(|part| {
                part.iter()
                    .any(|&j| geom::dist2(p, pts[j as usize]) < r * r)
            })
        })
    }

    /// Call `f` with each centreline segment that has an end among the
    /// points in the 3 × 3 block of grid cells around `p`, until it returns
    /// true; returns whether it did. A segment may be passed twice.
    fn any_segment_near(&self, p: P, g: Cells, mut f: impl FnMut(P, P) -> bool) -> bool {
        let (cx, cy) = g.cell(p);
        let pts = &self.centerline;
        let n = pts.len();
        (cy - 1..=cy + 1).any(|y| {
            self.row(g, cx, y).iter().any(|part| {
                part.iter().any(|&j| {
                    let j = j as usize;
                    (j > 0 && f(pts[j - 1], pts[j])) || (j + 1 < n && f(pts[j], pts[j + 1]))
                })
            })
        })
    }

    /// Whether a recent cut suppresses a cutoff at `p`.
    fn near_recent(&self, p: P) -> bool {
        let r = 0.5 * self.params.cutoff_min_sep as f64 * self.ds();
        self.recent_cuts.iter().any(|&(c, _)| dist(c, p) < r)
    }

    /// Widest local width along the centreline, in channel widths.
    fn max_width(&self) -> f64 {
        self.widths.iter().fold(MIN_WIDTH, |a, &b| a.max(b))
    }

    /// Size of the grid cells `capture` searches: banks within the cutoff
    /// distance of a lake as wide as the widest channel.
    fn capture_cell(&self) -> f64 {
        let d_cut = self.params.cutoff_dist * self.params.width;
        (0.5 * d_cut * (self.max_width() + MAX_WIDTH)).max(self.params.width)
    }

    /// Find one neck cutoff and apply it. Returns whether a cut was made.
    /// A neck closes when the centrelines come within `cutoff_dist` times
    /// the mean of their local widths, so the banks meet at a cutoff
    /// distance of 1.
    fn cutoff(&mut self) -> bool {
        let d_cut = self.params.cutoff_dist * self.params.width;
        let k_sep = self.params.cutoff_min_sep;
        let n = self.centerline.len();
        if n < k_sep + 2 || d_cut <= 0.0 {
            return false;
        }
        // In `capture`'s cells, which it searches next on the same
        // centreline unless a cut is made, so one grid serves both.
        let g = self.build_grid_of(d_cut * self.max_width(), self.capture_cell());
        let (pts, widths) = (&self.centerline, &self.widths);

        // Smallest upstream index first, then the smallest loop from it. A
        // recent cut nearby holds a neck back unless its banks have closed
        // to half the cutoff distance, when it would otherwise cross.
        for i in 0..n {
            let (mut best, mut closing) = (None, false);
            self.for_neighbours_after(pts[i], g, i + k_sep, |j| {
                let d = 0.5 * d_cut * (widths[i] + widths[j]);
                let d2 = geom::dist2(pts[i], pts[j]);
                if d2 < d * d {
                    closing |= d2 < 0.25 * d * d;
                    if best.is_none_or(|b| j < b) {
                        best = Some(j);
                    }
                }
            });
            if let Some(j) = best
                && (closing || (!self.near_recent(pts[i]) && !self.near_recent(pts[j])))
            {
                self.apply_cut(i, j);
                return true;
            }
        }
        false
    }

    /// Take up one open oxbow lake whose banks meet the channel's at two
    /// places, A upstream and B downstream, where the route through the lake
    /// is shorter than the channel from A to B by more than `capture_ratio`.
    /// The lake between A and B becomes channel, and the channel's route
    /// becomes a new oxbow, as at a cutoff. Banks meet as at a neck cutoff,
    /// within `cutoff_dist` times the mean of their widths. Each run of lake
    /// points that meets the channel is one connection, and the lake between
    /// two runs is the route through it, joined to the channel from the
    /// last point of one run and the first of the next. Among the captures
    /// that qualify, the one that shortens the river most for its length is
    /// made, provided the new route crosses neither the rest of the channel
    /// nor itself and turns less than a right angle at every point. A
    /// sharper turn folds into a loop as the channel migrates. The ends of
    /// the lake as cut off are plugged ([`END_PLUG`]), so the channel must
    /// break into it elsewhere. Returns whether a capture was made.
    fn capture(&mut self) -> bool {
        let p = &self.params;
        let (ratio, d_cut, k_sep) = (p.capture_ratio, p.cutoff_dist * p.width, p.cutoff_min_sep);
        let n = self.centerline.len();
        let open = |o: &Oxbow| o.is_lake() && o.fill < CAPTURE_FILL;
        if ratio <= 0.0 || d_cut <= 0.0 || n < k_sep + 2 || !self.oxbows.iter().any(open) {
            return false;
        }
        let ds = geom::length(&self.centerline) / (n - 1) as f64;
        let plug = END_PLUG * self.params.width;
        let g = self.build_grid(self.capture_cell());
        let (pts, widths) = (&self.centerline, &self.widths);
        let mut runs = std::mem::take(&mut self.contacts);
        // Captures that qualify, as (ratio, oxbow, A, B).
        let mut found: Vec<(f64, usize, Contact, Contact)> = Vec::new();
        for (oi, o) in self.oxbows.iter().enumerate().filter(|(_, o)| open(o)) {
            runs.clear();
            // The first and last contacts of the current run.
            let mut run: Option<(Contact, Contact)> = None;
            // Arc length along the oxbow as cut off.
            let mut s = o.offset;
            for (m, &q) in o.points.iter().enumerate() {
                if m > 0 {
                    s += dist(o.points[m - 1], q);
                }
                let mut meet: Option<Contact> = None;
                if s < plug || s > o.length - plug {
                    runs.extend(run.take());
                    continue;
                }
                // The fixed upstream end, index 0, cannot be taken up.
                self.for_neighbours_after(q, g, 0, |j| {
                    let (d2, r) = (
                        geom::dist2(q, pts[j]),
                        0.5 * d_cut * (widths[j] + o.widths[m]),
                    );
                    if d2 < r * r && meet.is_none_or(|c| d2 < c.2) {
                        meet = Some((m, j, d2));
                    }
                });
                match meet {
                    Some(c) => run = Some((run.map_or(c, |r| r.0), c)),
                    None => runs.extend(run.take()),
                }
            }
            runs.extend(run);
            for pair in runs.windows(2) {
                let (x, y) = (pair[0].1, pair[1].0);
                let (a, b) = if x.1 < y.1 { (x, y) } else { (y, x) };
                if b.1 - a.1 < k_sep || b.1 + 1 >= n {
                    continue;
                }
                let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
                let lake = a.2.sqrt() + geom::length(&o.points[lo..=hi]) + b.2.sqrt();
                let r = (b.1 - a.1) as f64 * ds / lake.max(1e-9);
                if r > ratio {
                    found.push((r, oi, a, b));
                }
            }
        }
        self.contacts = runs;
        found.sort_by(|x, y| y.0.total_cmp(&x.0));
        for &(_, oi, a, b) in &found {
            let route = self.capture_route(oi, a, b);
            if self.near_recent(route[0])
                || self.near_recent(route[route.len() - 1])
                || self.route_turns_sharply(a.1, b.1, &route)
                || self.route_crosses(a.1, b.1, &route)
            {
                continue;
            }
            self.apply_capture(oi, a, b);
            return true;
        }
        false
    }

    /// Scan the next [`CHUTE_SCAN`] points for a chute. From each point
    /// `i`, the bend to take is the point `j` downstream whose chord from
    /// `i` is between the [`CHUTE_CHORD`] lengths, with the bend between
    /// them at least [`CHUTE_RATIO`] times as long, and the longest bend
    /// for its chord. The flood cuts it with chance `chute_rate` times the
    /// share of the chord's ground that is bare bar and the sediment supply,
    /// over the time since
    /// the scan last passed: bare at growth 0, bound at `vegetation_time`.
    /// The chute must not cross the channel elsewhere or turn sharply at
    /// its ends, and the bend becomes an oxbow, as at a capture. Returns
    /// whether a chute was cut.
    fn chute(&mut self) -> bool {
        let p = &self.params;
        let n = self.centerline.len();
        let k_sep = p.cutoff_min_sep;
        if p.chute_rate <= 0.0 || n < k_sep + 2 {
            return false;
        }
        let (w, ds) = (p.width, self.ds());
        let (lo, hi) = (CHUTE_CHORD.0 * w, CHUTE_CHORD.1 * w);
        let reach = (CHUTE_ARC * w / ds) as usize;
        let span = p.dt * n.div_ceil(CHUTE_SCAN) as f64;
        let (rate, bound) = (
            p.chute_rate * self.supply(),
            p.vegetation_time.max(1e-9) as f32,
        );
        let from = self.chute_at.min(n);
        self.chute_at = if from + CHUTE_SCAN >= n {
            0
        } else {
            from + CHUTE_SCAN
        };
        let pts = &self.centerline;
        for i in (from..(from + CHUTE_SCAN).min(n)).filter(|&i| i > 0) {
            let mut best: Option<(f64, usize)> = None;
            for j in i + k_sep..(i + reach).min(n - 1) {
                let d = dist(pts[i], pts[j]);
                let r = (j - i) as f64 * ds / d.max(1e-9);
                if (lo..=hi).contains(&d) && r >= CHUTE_RATIO && best.is_none_or(|b| r > b.0) {
                    best = Some((r, j));
                }
            }
            let Some((_, j)) = best else { continue };
            let route = self.chute_route(i, j);
            let field = &self.erodibility;
            let bare = route
                .iter()
                .map(|&q| 1.0 - (field.growth[field.cell_index(q)] / bound).clamp(0.0, 1.0))
                .sum::<f32>() as f64
                / route.len() as f64;
            if bare <= 0.0
                || self.near_recent(pts[i])
                || self.near_recent(pts[j])
                || self.route_turns_sharply(i, j, &route)
                || self.route_crosses(i, j, &route)
                || self.events.uniform() >= 1.0 - (-rate * bare * span).exp()
            {
                continue;
            }
            let (wi, wj) = (self.widths[i], self.widths[j]);
            let m = route.len() - 1;
            let widths: Vec<f64> = (0..=m)
                .map(|k| wi + (wj - wi) * k as f64 / m as f64)
                .collect();
            let old = self.centerline[i..=j].to_vec();
            let old_widths = self.widths[i..=j].to_vec();
            self.centerline
                .splice(i + 1..j, route[1..m].iter().copied());
            self.widths.splice(i + 1..j, widths[1..m].iter().copied());
            self.line_version += 1;
            self.abandon(old, old_widths, route[1.min(m)]);
            self.chutes += 1;
            return true;
        }
        false
    }

    /// The straight chute from point `i` to point `j`, at the point
    /// spacing.
    fn chute_route(&self, i: usize, j: usize) -> Vec<P> {
        let (a, b) = (self.centerline[i], self.centerline[j]);
        let m = ((dist(a, b) / self.ds()).ceil() as usize).max(1);
        (0..=m)
            .map(|k| geom::lerp(a, b, k as f64 / m as f64))
            .collect()
    }

    /// The new channel from A to B through oxbow `oi`: A, the lake points
    /// from A's contact to B's, and B.
    fn capture_route(&self, oi: usize, a: Contact, b: Contact) -> Vec<P> {
        let o = &self.oxbows[oi].points;
        let mut route = vec![self.centerline[a.1]];
        if a.0 <= b.0 {
            route.extend_from_slice(&o[a.0..=b.0]);
        } else {
            route.extend(o[b.0..=a.0].iter().rev());
        }
        route.push(self.centerline[b.1]);
        route
    }

    /// Whether `route`, replacing the channel between points `i` and `j`,
    /// turns through a right angle or more at any point, including where it
    /// joins the channel either side. Points that coincide are skipped.
    fn route_turns_sharply(&self, i: usize, j: usize, route: &[P]) -> bool {
        let pts = &self.centerline;
        let mut prev: Option<P> = None;
        let mut last = pts[i - 1];
        for &q in route.iter().chain([&pts[j + 1]]) {
            let d = [q[0] - last[0], q[1] - last[1]];
            if d == [0.0, 0.0] {
                continue;
            }
            if prev.is_some_and(|u| u[0] * d[0] + u[1] * d[1] <= 0.0) {
                return true;
            }
            (prev, last) = (Some(d), q);
        }
        false
    }

    /// Whether `route`, replacing the channel between points `i` and `j`,
    /// crosses the channel outside them or itself.
    fn route_crosses(&self, i: usize, j: usize, route: &[P]) -> bool {
        let pts = &self.centerline;
        let n = pts.len();
        route.windows(2).enumerate().any(|(k, s)| {
            (0..n - 1)
                .filter(|&c| c < i || c >= j)
                .any(|c| segments_cross(s[0], s[1], pts[c], pts[c + 1]))
                || route[k + 2..]
                    .windows(2)
                    .any(|t| segments_cross(s[0], s[1], t[0], t[1]))
        })
    }

    /// Take up oxbow `oi` between contacts `a` and `b`: the channel runs
    /// through the lake from A to B, the lake's ends beyond them stay as
    /// lakes, and the channel's old route becomes a new oxbow.
    fn apply_capture(&mut self, oi: usize, a: Contact, b: Contact) {
        let (i, j) = (a.1, b.1);
        let route = self.capture_route(oi, a, b);
        let o = self.oxbows.swap_remove(oi);
        let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
        let mut route_widths = vec![self.widths[i]];
        if a.0 <= b.0 {
            route_widths.extend_from_slice(&o.widths[lo..=hi]);
        } else {
            route_widths.extend(o.widths[lo..=hi].iter().rev());
        }
        route_widths.push(self.widths[j]);
        // Each end left reaches the point now under the channel, as a piece
        // the channel splits does.
        for (s, e) in [(0, lo + 1), (hi, o.points.len())] {
            if e - s >= MIN_OXBOW_POINTS {
                let offset = o.offset + geom::length(&o.points[..=s]);
                self.oxbows.push(Oxbow {
                    points: o.points[s..e].to_vec(),
                    widths: o.widths[s..e].to_vec(),
                    offset,
                    ..o
                });
            }
        }
        let old = self.centerline[i..=j].to_vec();
        let old_widths = self.widths[i..=j].to_vec();
        self.centerline
            .splice(i + 1..j, route[1..route.len() - 1].iter().copied());
        self.widths.splice(
            i + 1..j,
            route_widths[1..route_widths.len() - 1].iter().copied(),
        );
        self.line_version += 1;
        self.abandon(old, old_widths, route[2.min(route.len() - 1)]);
        self.captures += 1;
    }

    /// Remove the parts of oxbows the channel has reached, splitting an
    /// oxbow where the channel crosses it, and drop pieces too short to
    /// draw. The channel cuts an oxbow where its centreline crosses one of
    /// the oxbow's segments, or comes within `REWORK_RADIUS` of one of its
    /// points. A point it comes that close to ends the pieces on both sides
    /// of it, and the two points of a crossed segment each end the piece on
    /// their side, so the lake runs in beneath the channel and the cut end
    /// stays hidden as the channel advances. A piece keeps its parent's
    /// birth time and length.
    fn rework_oxbows(&mut self) {
        if self.oxbows.is_empty() {
            return;
        }
        let w = self.params.width;
        let r2 = (REWORK_RADIUS * w).powi(2);
        let g = self.build_grid(w);
        let oxbows = std::mem::take(&mut self.oxbows);
        let mut kept = Vec::with_capacity(oxbows.len());
        // Pieces of the current oxbow, as ranges of point indices.
        let mut pieces: Vec<(usize, usize)> = Vec::new();
        let (mut near, mut hit) = (Vec::new(), Vec::new());
        let mut changed = false;
        for o in oxbows {
            let pts = &o.points;
            let n = pts.len();
            // Points spaced half a width apart, so a segment within reach of
            // a point, or crossing a segment, has an end within a width.
            near.clear();
            near.extend(pts.iter().map(|&q| self.near_channel(q, w, g)));
            if !near.contains(&true) {
                kept.push(o);
                continue;
            }
            hit.clear();
            hit.extend(pts.iter().zip(&near).map(|(&q, &close)| {
                close && self.any_segment_near(q, g, |a, b| geom::seg_dist2(q, a, b) < r2)
            }));
            // A channel point on the segment, as well as a crossing, cuts it.
            let crossed = |k: usize| {
                let (p, q) = (pts[k], pts[k + 1]);
                self.any_segment_near(geom::lerp(p, q, 0.5), g, |a, b| {
                    segments_cross(p, q, a, b)
                        || geom::seg_dist2(a, p, q) < r2
                        || geom::seg_dist2(b, p, q) < r2
                })
            };
            pieces.clear();
            let mut start = 0;
            for k in 0..n - 1 {
                // A hit point already ends the pieces either side of it.
                if (near[k] || near[k + 1]) && !hit[k] && !hit[k + 1] && crossed(k) {
                    pieces.push((start, k + 1));
                    start = k + 1;
                }
                if hit[k + 1] {
                    pieces.push((start, k + 2));
                    start = k + 1;
                }
            }
            pieces.push((start, n));
            // A hit only at an end, which lies under the channel at the
            // cut, keeps the whole oxbow as it was.
            pieces.retain(|&(a, b)| b - a >= MIN_OXBOW_POINTS);
            if pieces[..] == [(0, n)] {
                kept.push(o);
                continue;
            }
            changed = true;
            for &(a, b) in &pieces {
                let offset = o.offset + geom::length(&pts[..=a]);
                kept.push(Oxbow {
                    points: pts[a..b].to_vec(),
                    widths: o.widths[a..b].to_vec(),
                    offset,
                    ..o
                });
            }
        }
        self.oxbows = kept;
        if changed {
            self.oxbow_version += 1;
        }
    }

    /// Merge the older oxbows into a lake just cut off along `pts`, with the
    /// channel's widths `widths` there: the parts of them under its water
    /// become part of it. A channel can run over a lake without its
    /// centreline coming within `REWORK_RADIUS` of it, which hides the lake
    /// until the cut leaves both in the open. Each piece left reaches one
    /// point under the new lake, so their water joins, as a piece the
    /// channel splits reaches under the channel.
    fn absorb_oxbows(&mut self, pts: &[P], widths: &[f64]) {
        let half = 0.5 * self.params.width;
        let reach = half * widths.iter().fold(MIN_WIDTH, |a, &b| a.max(b));
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in pts {
            for d in 0..2 {
                lo[d] = lo[d].min(p[d] - reach);
                hi[d] = hi[d].max(p[d] + reach);
            }
        }
        let wet = |q: P| {
            (lo[0]..=hi[0]).contains(&q[0])
                && (lo[1]..=hi[1]).contains(&q[1])
                && pts.windows(2).zip(widths.windows(2)).any(|(s, b)| {
                    let r = half * b[0].max(b[1]);
                    geom::seg_dist2(q, s[0], s[1]) < r * r
                })
        };
        let oxbows = std::mem::take(&mut self.oxbows);
        let mut kept = Vec::with_capacity(oxbows.len());
        for o in oxbows {
            let under: Vec<bool> = o.points.iter().map(|&q| wet(q)).collect();
            if !under.contains(&true) {
                kept.push(o);
                continue;
            }
            // Runs of points clear of the new lake, with the point under it
            // either side.
            let n = o.points.len();
            let mut k = 0;
            while k < n {
                if under[k] {
                    k += 1;
                    continue;
                }
                let start = k;
                while k < n && !under[k] {
                    k += 1;
                }
                let (a, b) = (start.saturating_sub(1), (k + 1).min(n));
                if b - a >= MIN_OXBOW_POINTS {
                    let offset = o.offset + geom::length(&o.points[..=a]);
                    kept.push(Oxbow {
                        points: o.points[a..b].to_vec(),
                        widths: o.widths[a..b].to_vec(),
                        offset,
                        ..o
                    });
                }
            }
        }
        self.oxbows = kept;
    }

    /// Fill each lake by the bedload carried over time `dt` through an
    /// entrance near the channel, by [`Params::bedload_rate`]. The silt
    /// laid on its bed is read in [`Sim::step`]. Both settle into the fill
    /// over `SETTLE_TIME`.
    fn fill_lakes(&mut self, dt: f64) {
        let supply = self.supply();
        let p = &self.params;
        let (half, length) = (0.5 * p.width, p.levee_length * p.width);
        for o in self.oxbows.iter_mut().filter(|o| o.is_lake()) {
            // Only the piece holding the original upstream end has the entrance.
            let mut inflow = 0.0;
            if o.offset == 0.0 && o.entry > 0.0 && length > 0.0 {
                let gap = self
                    .centerline
                    .iter()
                    .map(|&c| geom::dist2(o.points[0], c))
                    .fold(f64::INFINITY, f64::min);
                inflow = supply
                    * dt
                    * p.bedload_rate
                    * o.entry
                    * (-(gap.sqrt() - half * o.widths[0]).max(0.0) / length).exp();
            }
            o.settling = (o.settling + inflow).min(1.0 - o.fill);
        }
    }

    /// Leave a scroll bar along the inner bank of each run of points whose
    /// bank is migrating, as high water does on the point bar it builds.
    fn add_scrolls(&mut self) {
        if self.params.scroll_lifetime <= 0.0 {
            return;
        }
        self.migration_rate();
        let w = self.params.width;
        let pts = &self.centerline;
        let n = pts.len();
        let mut run: Vec<P> = Vec::new();
        let mut side = 0.0;
        let mut added = Vec::new();
        for i in 1..n {
            let r = if i < n - 1 { self.rate[i] } else { 0.0 };
            let s = if r > SCROLL_MIN_RATE {
                1.0
            } else if r < -SCROLL_MIN_RATE {
                -1.0
            } else {
                0.0
            };
            if s != side {
                if run.len() >= MIN_SCROLL_POINTS {
                    added.push(Scroll {
                        points: std::mem::take(&mut run),
                        born: self.time,
                    });
                }
                run.clear();
                side = s;
            }
            if s != 0.0 {
                // A positive rate moves the point along its right normal,
                // so the inner bank lies along the left.
                let nrm = right_normal(pts, i);
                let off = (0.5 * self.widths[i] + SCROLL_OFFSET) * w;
                run.push([pts[i][0] - s * off * nrm[0], pts[i][1] - s * off * nrm[1]]);
            }
        }
        if !added.is_empty() {
            let p = &self.params;
            for s in &added {
                self.erodibility
                    .raise(&s.points, p.scroll_height, p.levee_height);
            }
            self.scrolls.extend(added);
            self.scroll_version += 1;
        }
    }

    /// Drop scroll bars past their lifetime, the parts of the rest within
    /// half a width of the channel, which has eroded them, and the parts
    /// buried under overbank deposits within [`BURY`] of the flood level,
    /// where the swales between the ridges have filled.
    fn rework_scrolls(&mut self) {
        let (time, life) = (self.time, self.params.scroll_lifetime);
        let before = self.scrolls.len();
        self.scrolls.retain(|s| time - s.born < life);
        let mut changed = self.scrolls.len() != before;
        if !self.scrolls.is_empty() {
            let w = self.params.width;
            let g = self.build_grid(w);
            let scrolls = std::mem::take(&mut self.scrolls);
            let mut kept = Vec::with_capacity(scrolls.len());
            let buried = BURY * self.params.levee_height;
            let field = &self.erodibility;
            for sc in scrolls {
                let wet = |q: P| field.deposit_at(q) > buried || self.near_channel(q, 0.5 * w, g);
                if !sc.points.iter().any(|&q| wet(q)) {
                    kept.push(sc);
                    continue;
                }
                changed = true;
                for piece in sc.points.split(|&q| wet(q)) {
                    if piece.len() >= MIN_SCROLL_POINTS {
                        kept.push(Scroll {
                            points: piece.to_vec(),
                            born: sc.born,
                        });
                    }
                }
            }
            self.scrolls = kept;
        }
        if changed {
            self.scroll_version += 1;
        }
    }

    /// Join point `i` to point `j` and abandon the reach between them as an
    /// oxbow.
    fn apply_cut(&mut self, i: usize, j: usize) {
        let loop_pts = self.centerline[i..=j].to_vec();
        let loop_widths = self.widths[i..=j].to_vec();
        let toward = self.centerline[j];
        self.centerline.drain(i + 1..j);
        self.widths.drain(i + 1..j);
        self.line_version += 1;
        self.abandon(loop_pts, loop_widths, toward);
    }

    /// Abandon the reach along `loop_pts`, which the channel has left, as an
    /// oxbow lake, where the new channel leaves its upstream end towards
    /// `toward`.
    fn abandon(&mut self, loop_pts: Vec<P>, loop_widths: Vec<f64>, toward: P) {
        // Diversion angle at the upstream end, between the new channel and
        // the oxbow's first segment.
        let (a, b, c) = (loop_pts[0], loop_pts[1], toward);
        let (main, branch) = ([c[0] - a[0], c[1] - a[1]], [b[0] - a[0], b[1] - a[1]]);
        let norms = (geom::dist2(a, c) * geom::dist2(a, b)).sqrt();
        let entry = if norms > 0.0 {
            ((main[0] * branch[0] + main[1] * branch[1]) / norms).max(0.0)
        } else {
            0.0
        };
        let mid = geom::lerp(a, loop_pts[loop_pts.len() - 1], 0.5);
        self.oxbow_version += 1;
        self.erodibility
            .stamp_oxbow(&loop_pts, &loop_widths, 0.5 * self.params.width, self.time);
        self.absorb_oxbows(&loop_pts, &loop_widths);
        self.oxbows
            .push(Oxbow::lake(loop_pts, loop_widths, self.time, entry));
        self.recent_cuts.push((mid, self.steps + CUT_COOLDOWN));
        self.cutoffs += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Straight channel along x with a sine perturbation, cutoffs off.
    fn sine_sim(amp_w: f64, wavelength_w: f64) -> Sim {
        // No wood or mills, whose jams and weirs would bend the sine.
        let params = Params {
            cutoff_dist: 0.0,
            jitter: 0.0,
            heterogeneity: 0.0,
            wood_rate: 0.0,
            mill_rate: 0.0,
            ..Params::default()
        };
        let w = params.width;
        let mut sim = Sim::new(params, [400.0 * w, 100.0 * w], 1, 0.0);
        let k = std::f64::consts::TAU / (wavelength_w * w);
        for p in &mut sim.centerline {
            p[1] += amp_w * w * (k * p[0]).sin();
        }
        sim
    }

    /// Amplitude and phase of the fundamental over the middle of the domain,
    /// from a projection onto sin and cos at the given wavelength.
    fn fundamental(sim: &Sim, wavelength_w: f64) -> (f64, f64) {
        let w = sim.params.width;
        let k = std::f64::consts::TAU / (wavelength_w * w);
        let (x0, x1) = (0.25 * sim.bounds[0], 0.75 * sim.bounds[0]);
        let mid = sim.bounds[1] / 2.0;
        let (mut s, mut c) = (0.0, 0.0);
        for win in sim.centerline.windows(2) {
            let x = win[0][0];
            if x < x0 || x > x1 {
                continue;
            }
            let dx = win[1][0] - x;
            s += (win[0][1] - mid) * (k * x).sin() * dx;
            c += (win[0][1] - mid) * (k * x).cos() * dx;
        }
        // y ≈ A sin(k(x - shift)) = A cos(k shift) sin(kx) - A sin(k shift) cos(kx).
        let amp = s.hypot(c) * 2.0 / (x1 - x0);
        let shift = (-c).atan2(s) / k;
        (amp, shift)
    }

    #[test]
    fn sine_perturbation_grows_and_drifts_downstream() {
        let wl = 15.0;
        let mut sim = sine_sim(0.2, wl);
        let (a0, _) = fundamental(&sim, wl);
        // Sum small phase increments so the shift is not wrapped to ±λ/2.
        let mut drift = 0.0;
        let (mut prev, half) = (fundamental(&sim, wl).1, 0.5 * wl * sim.params.width);
        for _ in 0..40 {
            sim.step_n(5);
            let s = fundamental(&sim, wl).1;
            drift += (s - prev + half).rem_euclid(2.0 * half) - half;
            prev = s;
        }
        let (a1, _) = fundamental(&sim, wl);
        eprintln!("amplitude {a0} -> {a1}, drift {drift}");
        assert!(a1 > 1.2 * a0, "amplitude {a0} -> {a1} should grow");
        assert!(
            drift > 0.5 * sim.params.width,
            "drift {drift} should be downstream (+x)"
        );
    }

    #[test]
    fn winding_reaches_are_gentler_and_migrate_more_slowly() {
        let wl = 15.0;
        let (mut gentle, mut winding) = (sine_sim(0.5, wl), sine_sim(3.0, wl));
        gentle.migration_rate();
        let with = winding.migration_rate().to_vec();
        let mid = winding.centerline.len() / 2;
        let (sg, sw) = (
            gentle.slope[gentle.centerline.len() / 2],
            winding.slope[mid],
        );
        // A sine of amplitude kA = 1.26 has sinuosity about 1.32.
        assert!(
            (sg - 1.0).abs() < 0.02 && (1.0 / sw - 1.32).abs() < 0.03,
            "slopes {sg} and {sw}"
        );
        winding.params.slope_exponent = 0.0;
        let without = winding.migration_rate().to_vec();
        // Compare where the rate is largest, away from its zero crossings.
        let i = (mid - 30..mid + 30)
            .max_by(|&a, &b| without[a].abs().total_cmp(&without[b].abs()))
            .unwrap();
        let expected = (SLOPE_SINUOSITY * winding.slope[i]).sqrt();
        assert!(
            (with[i] / without[i] - expected).abs() < 1e-9,
            "rate ratio {} should be {expected}",
            with[i] / without[i]
        );
        assert!(expected < 1.0);
    }

    #[test]
    fn local_only_does_not_drift() {
        let wl = 15.0;
        let mut sim = sine_sim(0.2, wl);
        sim.params.omega = 1.0;
        sim.params.gamma = 0.0;
        let (a0, s0) = fundamental(&sim, wl);
        sim.step_n(200);
        let (a1, s1) = fundamental(&sim, wl);
        assert!(a1 > a0);
        assert!(
            (s1 - s0).abs() < 0.1 * wl * sim.params.width,
            "shift {s0} -> {s1}, amplitude {a0} -> {a1}"
        );
    }

    #[test]
    fn noise_meanders_cuts_off_and_saturates() {
        let mut sim = Sim::new(Params::default(), [800.0, 480.0], 7, 0.5);
        let mut max_sin: f64 = 1.0;
        for _ in 0..200 {
            sim.step_n(100);
            max_sin = max_sin.max(sim.sinuosity());
            assert!(
                !geom::self_intersects(&sim.centerline),
                "self-intersection at step {}",
                sim.steps
            );
        }
        assert!(sim.cutoffs >= 3, "only {} cutoffs", sim.cutoffs);
        assert!(max_sin < 6.0, "sinuosity reached {max_sin}");
    }

    /// Mean sinuosity over the second half of a long run, and the mean arc
    /// length of the loops cut off over the whole run, in channel widths.
    fn late_sinuosity(params: Params, seed: u64) -> (f64, f64, Sim) {
        let mut sim = Sim::new(params, [1200.0, 480.0], seed, 0.5);
        let (mut sum, mut cut) = (0.0, Vec::new());
        for k in 0..300 {
            let before = sim.time;
            sim.step_n(100);
            if k >= 150 {
                sum += sim.sinuosity();
            }
            // Pieces of one oxbow share its birth time and length.
            for o in sim.oxbows.iter().filter(|o| o.born >= before) {
                if !cut.contains(&(o.born, o.length)) {
                    cut.push((o.born, o.length));
                }
            }
            assert!(
                !geom::self_intersects(&sim.centerline),
                "self-intersection at step {}",
                sim.steps
            );
        }
        let loop_w =
            cut.iter().map(|c| c.1).sum::<f64>() / cut.len().max(1) as f64 / sim.params.width;
        (sum / 150.0, loop_w, sim)
    }

    /// Joins a test thread, re-raising its panic so the failure message survives.
    fn join<T>(h: std::thread::ScopedJoinHandle<'_, T>) -> T {
        h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))
    }

    #[test]
    fn cutoffs_never_self_intersect() {
        // Loose point bars, overbank deposits and jams of wood change the
        // planform, so one run of each pair has them off to isolate the
        // cutoffs, and the other runs the defaults.
        let isolated = Params {
            bar_factor: 1.0,
            overbank_rate: 0.0,
            wood_rate: 0.0,
            ..Params::default()
        };
        // The four runs are independent, so each gets its own thread.
        std::thread::scope(|s| {
            let runs: Vec<_> = [3, 11]
                .into_iter()
                .flat_map(|seed| [(seed, isolated.clone()), (seed, Params::default())])
                .map(|(seed, params)| (seed, s.spawn(move || late_sinuosity(params, seed))))
                .collect();
            for (seed, run) in runs {
                let (sinuosity, loops, sim) = join(run);
                eprintln!(
                    "seed {seed}: sinuosity {sinuosity:.2}, {} cutoffs, loops cut at {loops:.1} widths",
                    sim.cutoffs
                );
                assert!(sim.cutoffs >= 3, "only {} cutoffs", sim.cutoffs);
            }
        });
    }

    #[test]
    fn no_building_stands_across_a_stream() {
        for seed in [2, 7, 11] {
            let (checked, segments, houseless) = streams_and_buildings(seed);
            eprintln!(
                "seed {seed}: {checked} buildings checked, {segments} stream segments, {houseless} holdings without a house"
            );
            assert!(segments > 0 && checked > 0);
            // A house moves off a stream beside its seat, and a holding
            // that loses its house rebuilds by the next survey, so few
            // holdings go without one. Over seeds 1 to 12 the count runs
            // from 0 to 4, mostly houses the water took at that survey.
            assert!(houseless <= 5, "{houseless} holdings without a house");
        }
    }

    /// Run seed `seed`, checking right after each survey's weathering that
    /// no building stands across a stream. Returns the buildings checked,
    /// the stream segments at the end, and the holdings found without a
    /// house, summed over the checks.
    fn streams_and_buildings(seed: u64) -> (usize, usize, usize) {
        let mut sim = Sim::new(Params::default(), [900.0, 480.0], seed, 0.5);
        let w = sim.params.width;
        let (mut checked, mut houseless) = (0, 0);
        for _ in 0..8000 {
            sim.step();
            // Right after a survey's weathering, with the streams as it saw them.
            if sim.steps % land::SURVEY_STEPS != 2 * land::BANDS as u64 {
                continue;
            }
            let across = sim
                .land
                .buildings
                .iter()
                .filter(|b| sim.land.across_stream(b, w))
                .count();
            assert_eq!(
                across, 0,
                "{across} buildings across a stream at step {}",
                sim.steps
            );
            checked += sim.land.buildings.len();
            houseless += (0..sim.land.holdings.len())
                .filter(|&h| sim.land.holdings[h].is_some())
                .filter(|&h| {
                    !sim.land
                        .buildings
                        .iter()
                        .any(|b| b.holding == h as u32 && b.kind == farm::Kind::House)
                })
                .count();
        }
        let segments = sim.land.streams.iter().map(|r| r.segments.len()).sum();
        (checked, segments, houseless)
    }

    #[test]
    fn farmsteads_leave_the_river_and_holdings_unchanged() {
        let run = |farmsteads| {
            let params = Params {
                farmsteads,
                hamlet_chance: 0.0,
                ..Params::default()
            };
            let mut sim = Sim::new(params, [900.0, 480.0], 2, 0.5);
            for _ in 0..4000 {
                sim.step();
            }
            sim
        };
        let (off, on) = (run(false), run(true));
        assert!(off.land.buildings.is_empty());
        assert!(on.land.buildings.len() > on.land.count());
        assert_eq!(off.centerline, on.centerline);
        assert_eq!(off.land.owner, on.land.owner);
    }

    #[test]
    fn churches_stand_apart_and_ponds_lie_in_old_channels() {
        let (mut churches, mut ponds) = (0, 0);
        for seed in [1, 3] {
            let mut sim = Sim::new(Params::default(), [900.0, 480.0], seed, 0.5);
            for _ in 0..12000 {
                sim.step();
            }
            let (land, w) = (&sim.land, sim.params.width);
            let standing: Vec<_> = (land.buildings.iter())
                .filter(|b| b.kind == farm::Kind::Church && b.standing())
                .collect();
            for (k, a) in standing.iter().enumerate() {
                assert_eq!(a.dir, [1.0, 0.0], "a church laid east and west");
                assert_eq!(a.holding, land::NONE, "a church no holding keeps");
                for b in &standing[k + 1..] {
                    assert!(dist(a.pos, b.pos) >= farm::CHURCH_GAP * w);
                }
            }
            for p in &land.ponds {
                let i = land.index(p.pos);
                assert!(sim.erodibility.cut_at[i].is_finite(), "a pond off the plug");
                if p.kept() {
                    let seat = land.holdings[p.holding as usize].as_ref().map(|g| g.seat);
                    assert!(seat.is_some(), "a pond kept by no holding");
                }
            }
            churches += standing.len();
            ponds += land.ponds.len();
        }
        eprintln!("{churches} churches and {ponds} ponds over two runs of 3,000 years");
        assert!(churches >= 1 && ponds >= 1);
    }

    #[test]
    fn lanes_wear_into_hollow_ways_that_outlast_them() {
        let mut sim = Sim::new(Params::default(), [900.0, 480.0], 2, 0.5);
        for _ in 0..6000 {
            sim.step();
        }
        let tracks = &sim.tracks;
        let depths: Vec<f32> = tracks.depths.iter().flatten().copied().collect();
        assert!(depths.iter().all(|d| (0.0..=1.0).contains(d)));
        let deep = depths.iter().filter(|&&d| d > 0.5).count();
        assert!(
            deep > depths.len() / 4,
            "{deep} of {} cells deep",
            depths.len()
        );
        assert!(!tracks.hollows.is_empty(), "no abandoned hollow ways");
        let on: std::collections::BTreeSet<_> = (tracks.tracks.iter().flatten())
            .map(|p| (p[0].to_bits(), p[1].to_bits()))
            .collect();
        for (h, d) in tracks.hollows.iter().zip(&tracks.hollow_depths) {
            assert!(h.len() >= 2 && h.len() == d.len());
            for (p, &d) in h.iter().zip(d) {
                assert!(d > 0.0 && d <= 1.0);
                assert!(
                    !on.contains(&(p[0].to_bits(), p[1].to_bits())),
                    "a hollow way on a track"
                );
            }
        }
    }

    #[test]
    fn tracks_join_the_farmsteads_and_ford_the_river() {
        let mut sim = Sim::new(Params::default(), [900.0, 480.0], 2, 0.5);
        for _ in 0..2000 {
            sim.step();
        }
        let (tracks, w) = (&sim.tracks, sim.params.width);
        assert!(
            tracks.tracks.len() >= sim.land.count() / 2,
            "{} tracks",
            tracks.tracks.len()
        );
        // Every track joins one network: union its two ends.
        let key = |p: P| ((p[0] / w).round() as i64, (p[1] / w).round() as i64);
        let mut parent = std::collections::BTreeMap::new();
        fn root(
            parent: &mut std::collections::BTreeMap<(i64, i64), (i64, i64)>,
            k: (i64, i64),
        ) -> (i64, i64) {
            let up = *parent.entry(k).or_insert(k);
            if up == k {
                k
            } else {
                let r = root(parent, up);
                parent.insert(k, r);
                r
            }
        }
        for t in &tracks.tracks {
            let (a, b) = (
                root(&mut parent, key(t[0])),
                root(&mut parent, key(t[t.len() - 1])),
            );
            parent.insert(a, b);
        }
        let first = root(&mut parent, key(tracks.tracks[0][0]));
        for t in &tracks.tracks {
            assert_eq!(root(&mut parent, key(t[0])), first, "one network");
        }
        // The river splits the valley, so the network fords it, from dry
        // ground straight across.
        assert!(!tracks.fords.is_empty());
        for &[a, b] in &tracks.fords {
            let (i, j) = (sim.land.index(a), sim.land.index(b));
            assert!(!sim.land.in_channel(i) && !sim.land.in_channel(j));
            assert!(
                dist(a, b) < 4.0 * w,
                "a ford {} widths long",
                dist(a, b) / w
            );
        }
    }

    #[test]
    fn pinches_hold_one_crossing_near_the_axis() {
        let w = Params::default().width;
        let (up, view) = (60.0 * w, 840.0);
        std::thread::scope(|s| {
            let runs: Vec<_> = [1, 2]
                .into_iter()
                .map(|seed| {
                    s.spawn(move || {
                        let mut sim =
                            Sim::new(Params::default(), [up + view + 20.0 * w, 480.0], seed, 0.5);
                        sim.pinches = vec![up, up + view];
                        for _ in 0..30 {
                            sim.step_n(1000);
                            for &px in &sim.pinches {
                                let crossings: Vec<f64> = sim
                                    .centerline
                                    .windows(2)
                                    .filter(|s| (s[0][0] < px) != (s[1][0] < px))
                                    .map(|s| s[0][1])
                                    .collect();
                                assert_eq!(
                                    crossings.len(),
                                    1,
                                    "seed {seed}, step {}: crossings at x = {px}: {crossings:?}",
                                    sim.steps
                                );
                                let off = (crossings[0] - 240.0).abs();
                                assert!(
                                    off < 0.5 * sim.params.pinch_width * w + w,
                                    "seed {seed}, step {}: crossing {off} off axis",
                                    sim.steps
                                );
                            }
                        }
                    })
                })
                .collect();
            runs.into_iter().for_each(join);
        });
    }

    #[test]
    fn works_keep_to_the_window() {
        let mut sim = Sim::new(Params::default(), [1.0, 1.0], 0, 0.5);
        let view = 640.0;
        let x = sim.reset_view(view, 480.0, 3, 0.5);
        // 6,000 years: past the parish boundary and the first churches.
        sim.step_n(24_000);
        let (w, cell) = (sim.params.width, sim.land.cell);
        let (lo, hi) = (
            x - WINDOW_MARGIN * w - cell,
            x + view + WINDOW_MARGIN * w + cell,
        );
        let inside = |p: P, pad: f64| p[0] >= lo - pad && p[0] <= hi + pad;
        let land = &sim.land;
        assert!(land.count() > 0 && !sim.tracks.tracks.is_empty());
        for (i, &o) in land.owner.iter().enumerate() {
            assert!(
                o == land::NONE || inside(land.centre(i), 0.0),
                "held cell at {:?}",
                land.centre(i)
            );
        }
        for b in &land.buildings {
            assert!(inside(b.pos, 0.0), "{:?} at {:?}", b.kind, b.pos);
        }
        for p in &land.ponds {
            assert!(inside(p.pos, 0.0), "pond at {:?}", p.pos);
        }
        for &p in sim.tracks.tracks.iter().flatten() {
            assert!(inside(p, 0.0), "track at {p:?}");
        }
        // A leat needs most of its ground held, not all of it.
        for m in &sim.mills {
            assert!(
                inside(m.weir, (LEAT_UP + LEAT_DOWN) * w),
                "mill at {:?}",
                m.weir
            );
        }
    }

    #[test]
    fn physics_sets_the_kernel_weights() {
        let mut p = Params {
            physics: false,
            ..Params::default()
        };
        let alpha = p.alpha;
        p.set("friction", 0.02);
        assert_eq!(
            p.alpha, alpha,
            "hand-tuned weights stay until physics is on"
        );
        p.set("physics", 1.0);
        assert_eq!(p.get("physics"), Some(1.0));
        assert_eq!(p.omega, -1.0);
        assert!((p.gamma - 0.5 * (0.3f64.powi(2) + 2.89 + 3.0)).abs() < 1e-12);
        assert!((p.alpha - 2.0 * 0.02 * 30.0).abs() < 1e-12);
        p.set("gamma", 9.0);
        assert!(p.gamma < 4.0, "derived weights override direct sets");
        p.set("aspect", 10.0);
        assert!((p.alpha - 0.4).abs() < 1e-12);
    }

    #[test]
    fn lakes_near_the_channel_fill_first() {
        // Silt and bedload at 50 times their default rates.
        let mut sim = sine_sim(0.0, 40.0);
        sim.params.overbank_rate = 0.15;
        sim.params.bedload_rate = 0.1;
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // Straight lakes beside the channel, near and far, and one whose
        // entrance lies on the channel at a small diversion angle.
        let lake = |x0: f64, y: f64| -> Vec<P> {
            (0..30)
                .map(|k| [(x0 + 0.5 * k as f64) * w, mid + y * w])
                .collect()
        };
        for (pts, entry) in [
            (lake(100.0, 2.0), 0.0),
            (lake(150.0, 10.0), 0.0),
            (lake(200.0, 0.0), 0.9),
        ] {
            let widths = vec![1.0; pts.len()];
            sim.erodibility.stamp_oxbow(&pts, &widths, 0.5 * w, 0.0);
            sim.oxbows.push(Oxbow::lake(pts, widths, 0.0, entry));
        }
        // The third lake runs along the channel; keep only its entrance there.
        sim.oxbows[2]
            .points
            .iter_mut()
            .skip(1)
            .for_each(|q| q[1] += 3.0 * w);
        let fills = |sim: &Sim| sim.oxbows.iter().map(|o| o.fill).collect::<Vec<_>>();
        sim.step_n(1000);
        let f = fills(&sim);
        eprintln!("fills: {f:?}");
        let quiet = sim.time / sim.params.oxbow_lifetime;
        assert!(
            (f[1] - quiet).abs() < 0.05,
            "a far lake fills only by the clock: {} against {quiet}",
            f[1]
        );
        assert!(f[0] > f[1] + 0.2, "a near lake silts up faster");
        assert!(
            f[2] > f[1] + 0.2,
            "a lake with an open entrance takes in bedload"
        );
    }

    #[test]
    fn scroll_bars_lie_inside_migrating_bends() {
        let mut sim = sine_sim(2.0, 30.0);
        sim.params.scroll_time = 0.0;
        sim.params.overbank_rate = 0.0;
        sim.step_n(50);
        sim.add_scrolls();
        assert!(!sim.scrolls.is_empty());
        let w = sim.params.width;
        let mid = sim.bounds[1] / 2.0;
        let line = sim.centerline.clone();
        let gap = |q: P| {
            line.iter()
                .map(|&c| dist(q, c))
                .fold(f64::INFINITY, f64::min)
        };
        let widths = sim.widths.clone();
        let half = |q: P| {
            let k = (0..line.len())
                .min_by(|&a, &b| dist(line[a], q).total_cmp(&dist(line[b], q)))
                .unwrap();
            0.5 * widths[k] * w
        };
        let mut bends = 0;
        for sc in &sim.scrolls {
            assert!(sc.points.len() >= MIN_SCROLL_POINTS);
            for &q in &sc.points {
                assert!(
                    gap(q) > half(q) && gap(q) < half(q) + 0.2 * w,
                    "a ridge lies just off the bank"
                );
            }
            // Bends grow outwards, so the ridge at a bend lies on its axis
            // side: inside the bend.
            let k = sc.points.len() / 2;
            let c = line
                .iter()
                .min_by(|a, b| dist(**a, sc.points[k]).total_cmp(&dist(**b, sc.points[k])))
                .unwrap();
            if (c[1] - mid).abs() > 1.5 * w {
                bends += 1;
                assert!(
                    (sc.points[k][1] - mid).abs() < (c[1] - mid).abs(),
                    "ridge at {:?}, channel at {c:?}",
                    sc.points[k]
                );
            }
        }
        assert!(bends >= 4, "only {bends} ridges at bends");
        // The channel erodes a ridge it crosses.
        let v = sim.scroll_version;
        let across: Vec<P> = (-10..=10)
            .map(|k| [100.0 * w, mid + 0.5 * k as f64 * w])
            .collect();
        sim.scrolls = vec![Scroll {
            points: across,
            born: sim.time,
        }];
        sim.rework_scrolls();
        assert!(sim.scroll_version > v);
        for sc in &sim.scrolls {
            assert!(sc.points.iter().all(|&q| gap(q) >= 0.5 * w));
        }
    }

    #[test]
    fn a_reach_beside_a_valley_wall_can_leave_it() {
        // Tapering at the centreline slowed motion away from a wall as much
        // as motion into it, so a reach that drifted there stayed straight.
        let mut sim = sine_sim(0.0, 40.0);
        let p = sim.params.clone();
        let pts: Vec<P> = sim
            .centerline
            .iter()
            .map(|q| [q[0], sim.floor(q[0], true).1 - 2.0 * p.width])
            .collect();
        sim.centerline = pts;
        let (i, ground) = (sim.centerline.len() / 2, sim.ground());
        let nrm = right_normal(&sim.centerline, i);
        // The bank on the side `sign` of the normal, and its erodibility.
        let read = |sign: f64| {
            (
                sim.centerline[i][1] + sign * nrm[1],
                sim.bank_erodibility(i, nrm, sign, &ground),
            )
        };
        let (toward, away) = if read(1.0).0 > read(-1.0).0 {
            (read(1.0).1, read(-1.0).1)
        } else {
            (read(-1.0).1, read(1.0).1)
        };
        let here = edge_taper(
            sim.centerline[i],
            sim.bounds,
            p.edge_margin * p.width,
            sim.floor(sim.centerline[i][0], true),
        );
        assert!(
            away > here && here > toward,
            "away {away}, at the centreline {here}, toward {toward}"
        );
    }

    #[test]
    fn channel_reworks_oxbows_it_crosses() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // A vertical oxbow across the channel, and one well clear of it.
        let across: Vec<P> = (-20..=20)
            .map(|k| [100.0 * w, mid + 0.5 * k as f64 * w])
            .collect();
        let clear: Vec<P> = (0..20)
            .map(|k| [(150 + k) as f64 * w, mid + 20.0 * w])
            .collect();
        sim.oxbows
            .push(Oxbow::lake(across, vec![1.0; 41], 0.0, 0.0));
        sim.oxbows
            .push(Oxbow::lake(clear.clone(), vec![1.0; 20], 0.0, 0.0));
        sim.step();
        assert_eq!(sim.oxbows.len(), 3, "the crossed oxbow splits in two");
        assert_eq!(sim.oxbows[2].points, clear);
        // Each piece reaches one point under the water at the cut, and
        // keeps its place along the original oxbow.
        let gap = |q: P| {
            sim.centerline
                .iter()
                .map(|&c| dist(q, c))
                .fold(f64::INFINITY, f64::min)
        };
        let (a, b) = (&sim.oxbows[0], &sim.oxbows[1]);
        for o in [a, b] {
            let m = o.points.len();
            assert_eq!(o.widths.len(), m, "a piece keeps its share of the widths");
            assert!(o.points[1..m - 1].iter().all(|&q| gap(q) >= 0.5 * w));
        }
        assert!(gap(a.points[a.points.len() - 1]) < 0.5 * w && gap(b.points[0]) < 0.5 * w);
        assert!((b.offset - dist(b.points[0], [100.0 * w, mid - 10.0 * w])).abs() < 1e-9);
    }

    #[test]
    fn channel_splits_an_oxbow_between_its_points() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // The channel runs between two points a quarter width either side.
        let across: Vec<P> = (-20..20)
            .map(|k| [100.0 * w, mid + (0.5 * k as f64 + 0.25) * w])
            .collect();
        sim.oxbows
            .push(Oxbow::lake(across, vec![1.0; 40], 0.0, 0.0));
        sim.step();
        assert_eq!(sim.oxbows.len(), 2, "the crossed oxbow splits in two");
        assert_eq!(sim.oxbows[0].points.len() + sim.oxbows[1].points.len(), 40);
    }

    /// Mean width over the points migrating fastest and slowest, by the top
    /// and bottom quarter of the migration rate.
    fn width_by_rate(sim: &mut Sim) -> (f64, f64) {
        let rate: Vec<f64> = sim.migration_rate().iter().map(|r| r.abs()).collect();
        let mut order: Vec<usize> = (1..rate.len() - 1).collect();
        order.sort_by(|&a, &b| rate[a].total_cmp(&rate[b]));
        let q = order.len() / 4;
        let mean = |ix: &[usize]| ix.iter().map(|&i| sim.widths[i]).sum::<f64>() / ix.len() as f64;
        (mean(&order[order.len() - q..]), mean(&order[..q]))
    }

    #[test]
    fn bends_widen_into_soft_banks_and_narrow_into_clay() {
        let mut sim = sine_sim(2.0, 30.0);
        sim.params.overbank_rate = 0.0;
        sim.step_n(300);
        let (fast, slow) = width_by_rate(&mut sim);
        eprintln!("soft banks: width {fast:.2} where migrating fastest, {slow:.2} slowest");
        assert!(
            fast > 1.1 && fast > slow + 0.1,
            "the bank pulls ahead of the bar at a bend"
        );

        // Old clay plugs everywhere erode slower than the bar advances.
        let mut sim = sine_sim(2.0, 30.0);
        sim.params.overbank_rate = 0.0;
        sim.erodibility.cut_at.fill(-1e9);
        sim.step_n(300);
        let (fast, slow) = width_by_rate(&mut sim);
        eprintln!("clay banks: width {fast:.2} where migrating fastest, {slow:.2} slowest");
        assert!(
            fast < 0.95 && fast < slow - 0.03,
            "the bar pushes ahead of a clay bank"
        );
        assert!(
            sim.widths
                .iter()
                .all(|&b| (MIN_WIDTH..=MAX_WIDTH).contains(&b))
        );

        // A width time of 0 holds the reference width.
        let mut sim = sine_sim(2.0, 30.0);
        sim.params.width_time = 0.0;
        sim.step_n(300);
        assert!(sim.widths.iter().all(|&b| b == 1.0));
    }

    #[test]
    fn a_neck_closes_when_the_banks_meet() {
        // A hairpin whose legs run 1.3 widths apart, past the separation K.
        let mut sim = sine_sim(0.0, 40.0);
        sim.params.cutoff_dist = 1.0;
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        let r = 0.65 * w;
        let mut pts: Vec<P> = (0..30)
            .map(|k| [(100.0 + 0.5 * k as f64) * w, mid])
            .collect();
        let x = pts[29][0];
        pts.extend((1..4).map(|k| {
            let t = std::f64::consts::PI * k as f64 / 4.0;
            [x + r * t.sin(), mid + r - r * t.cos()]
        }));
        pts.extend(
            (0..30)
                .rev()
                .map(|k| [(100.0 + 0.5 * k as f64) * w, mid + 2.0 * r]),
        );
        sim.centerline = pts;
        sim.widths = vec![1.0; sim.centerline.len()];
        sim.line_version += 1;
        assert!(
            !sim.cutoff(),
            "banks of the reference width are 0.3 widths apart"
        );
        sim.widths.fill(1.5);
        sim.line_version += 1;
        assert!(sim.cutoff(), "banks 1.5 widths wide overlap");
        assert_eq!(sim.widths.len(), sim.centerline.len());
        let o = &sim.oxbows[0];
        assert!(
            o.widths.len() == o.points.len() && o.widths.iter().all(|&b| b == 1.5),
            "the lake keeps the channel's width"
        );
        let side = [
            o.points[o.points.len() / 2][0],
            o.points[o.points.len() / 2][1] + 0.65 * w,
        ];
        assert!(
            sim.erodibility.lake_silt_at(side).is_some(),
            "the footprint spans the wider channel"
        );
    }

    #[test]
    fn a_cutoff_absorbs_the_lake_under_it() {
        // The hairpin of `a_neck_closes_when_the_banks_meet`, with an old
        // lake running out from its upper leg.
        let mut sim = sine_sim(0.0, 40.0);
        sim.params.cutoff_dist = 1.0;
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        let r = 0.65 * w;
        let mut pts: Vec<P> = (0..30)
            .map(|k| [(100.0 + 0.5 * k as f64) * w, mid])
            .collect();
        let x = pts[29][0];
        pts.extend((1..4).map(|k| {
            let t = std::f64::consts::PI * k as f64 / 4.0;
            [x + r * t.sin(), mid + r - r * t.cos()]
        }));
        pts.extend(
            (0..30)
                .rev()
                .map(|k| [(100.0 + 0.5 * k as f64) * w, mid + 2.0 * r]),
        );
        sim.centerline = pts;
        sim.widths = vec![1.5; sim.centerline.len()];
        let old: Vec<P> = (0..=10)
            .map(|k| [105.0 * w, mid + 2.0 * r + 0.5 * k as f64 * w])
            .collect();
        let clear: Vec<P> = (0..20)
            .map(|k| [(150 + k) as f64 * w, mid + 20.0 * w])
            .collect();
        sim.oxbows
            .push(Oxbow::lake(old.clone(), vec![1.0; 11], 0.0, 0.0));
        sim.oxbows
            .push(Oxbow::lake(clear.clone(), vec![1.0; 20], 0.0, 0.0));
        assert!(sim.cutoff());
        assert_eq!(sim.oxbows.len(), 3);
        assert_eq!(
            sim.oxbows[1].points, clear,
            "a lake clear of the cut is left whole"
        );
        // The new lake is 0.75 widths in half-width, so it covers the first
        // two points; the piece left reaches back to the second.
        let piece = &sim.oxbows[0];
        assert_eq!(piece.points, old[1..]);
        assert_eq!(piece.widths.len(), piece.points.len());
        assert!((piece.offset - 0.5 * w).abs() < 1e-9);
    }

    #[test]
    fn clearance_raises_the_supply_and_the_silt_laid_overbank() {
        let mut sim = sine_sim(0.0, 40.0);
        let p = sim.params.clone();
        sim.time = p.clearance_start - 1.0;
        assert_eq!(sim.supply(), 1.0);
        sim.time = p.clearance_start + 0.5 * p.clearance_time;
        assert!((sim.supply() - 0.5 * (1.0 + p.clearance_supply)).abs() < 1e-9);
        sim.time = p.clearance_start + p.clearance_time + 1.0;
        assert_eq!(sim.supply(), p.clearance_supply);
        // One cycle of bands of overbank silt, before and after clearance.
        let run = |time: f64| {
            let mut sim = sine_sim(0.0, 40.0);
            sim.time = time;
            sim.step_n(DEPOSIT_EVERY as usize);
            let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
            [1.5, 5.0].map(|y| sim.erodibility.deposit_at([100.0 * w, mid + y * w]))
        };
        let (before, after) = (run(0.0), run(2000.0));
        assert!(
            after[0] > before[0],
            "more silt by the bank: {after:?} against {before:?}"
        );
        assert!(
            after[1] > 2.0 * before[1],
            "and carried farther out: {after:?} against {before:?}"
        );
    }

    #[test]
    fn a_jam_pushes_the_channel_off_its_bank() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, i) = (sim.params.width, sim.centerline.len() / 2);
        let (q, nrm) = (sim.centerline[i], right_normal(&sim.centerline, i));
        let pos = [q[0] + 0.5 * w * nrm[0], q[1] + 0.5 * w * nrm[1]];
        sim.jams.push(Jam {
            pos,
            down: [-nrm[1], nrm[0]],
            side: 1.0,
            size: 1.0,
            until: 1e9,
            at: i,
        });
        sim.step_n(100);
        assert_eq!(sim.jams.len(), 1, "the jam keeps to the bank");
        assert_eq!(sim.jams[0].pos, pos, "where it lodged");
        let p = sim
            .centerline
            .iter()
            .min_by(|a, b| (a[0] - q[0]).abs().total_cmp(&(b[0] - q[0]).abs()))
            .unwrap();
        let moved = (p[0] - q[0]) * nrm[0] + (p[1] - q[1]) * nrm[1];
        assert!(
            moved < -0.05 * w,
            "the channel moves off the jam's bank: {moved}"
        );
        let far = sim
            .centerline
            .iter()
            .find(|c| (c[0] - 50.0 * w).abs() < 0.3 * w)
            .unwrap();
        assert!((far[1] - q[1]).abs() < 1e-6, "and stays put away from it");
    }

    #[test]
    fn a_bank_against_a_wall_cuts_it_back_and_slumps() {
        let mut sim = sine_sim(0.0, 40.0);
        let w = sim.params.width;
        let pts: Vec<P> = sim
            .centerline
            .iter()
            .map(|q| [q[0], sim.floor(q[0], false).1 - 2.0 * w])
            .collect();
        sim.centerline = pts;
        let n = sim.centerline.len();
        let towards = right_normal(&sim.centerline, n / 2)[1].signum();
        let (foot, versions) = (sim.floor(200.0 * w, false).1, sim.wall_version);
        for _ in 0..2000 {
            sim.rate = vec![towards * 0.5; n];
            sim.erode_bluffs();
        }
        let back = sim.walls.retreat(1, 200.0);
        assert!(back > 0.5 && back <= BLUFF_MAX, "the wall retreats: {back}");
        assert_eq!(sim.walls.retreat(0, 200.0), 0.0, "the far wall stands");
        assert!(sim.floor(200.0 * w, false).1 > foot, "the floor widens");
        assert!(sim.wall_version > versions, "slumps redraw the wall");
        let lobe = [200.0 * w, sim.floor(200.0 * w, false).1];
        let coarse = (0..8).map(|k| {
            sim.erodibility.coarse[sim
                .erodibility
                .cell_index([lobe[0] + k as f64 * w, lobe[1]])]
        });
        assert!(coarse.fold(0.0f32, f32::max) > 0.3, "colluvium at the foot");
    }

    #[test]
    fn tributaries_build_fans_and_feed_the_bars_below() {
        let mut sim = Sim::new(Params::default(), [400.0 * 8.0, 60.0 * 8.0], 1, 0.5);
        assert!(!sim.tributaries.is_empty());
        let coarse = |p: P| sim.erodibility.coarse[sim.erodibility.cell_index(p)] as f64;
        for t in &sim.tributaries {
            let c = coarse(t.mouth);
            assert!(
                c >= t.scale - 0.1,
                "coarse at the apex: {c} for scale {}",
                t.scale
            );
            let half = [t.mouth[0] + 0.5 * t.radius, t.mouth[1]];
            assert!(
                coarse(half) >= 0.5f64.sqrt() * t.scale - 0.1,
                "falling towards the rim"
            );
        }
        // Clear of every fan, the ground holds no gravel.
        let clear = |p: P| {
            sim.tributaries
                .iter()
                .all(|t| dist(p, t.mouth) > t.radius + sim.erodibility.cell)
        };
        let mid = sim.bounds[1] / 2.0;
        assert!(
            (0..400)
                .map(|x| [(x as f64 + 0.5) * 8.0, mid])
                .filter(|&p| clear(p))
                .all(|p| coarse(p) == 0.0)
        );
        let fed = sim.migration_rate().to_vec();
        sim.params.tributary_boost = 0.0;
        let plain = sim.migration_rate().to_vec();
        let t = sim
            .tributaries
            .iter()
            .max_by(|a, b| a.scale.total_cmp(&b.scale))
            .unwrap();
        let r = sim.reach_points(TRIBUTARY_REACH);
        let k = (t.at + 1..t.at + r)
            .find(|&k| plain[k].abs() > 1e-6)
            .unwrap();
        // Confluences close together each feed the reach below them.
        let want = sim
            .tributaries
            .iter()
            .filter(|u| (u.at..u.at + r).contains(&k))
            .map(|u| 1.0 + TRIBUTARY_BOOST * u.scale * (1.0 - (k - u.at) as f64 / r as f64))
            .product::<f64>();
        assert!(
            (fed[k] / plain[k] - want).abs() < 1e-9,
            "boost {} against {want}",
            fed[k] / plain[k]
        );
        let first = sim.tributaries.iter().map(|u| u.at).min().unwrap();
        if let Some(k) = (0..first).rev().find(|&k| plain[k].abs() > 1e-6) {
            assert_eq!(fed[k], plain[k], "nothing above the confluences");
        }
    }

    #[test]
    fn a_weir_holds_its_reach_and_ponds_the_one_above() {
        let mut sim = sine_sim(2.0, 40.0);
        let (w, n) = (sim.params.width, sim.centerline.len());
        let i = n / 2;
        let free = sim.migration_rate().to_vec();
        let weir = sim.centerline[i];
        sim.mills.push(Mill {
            weir,
            across: [0.0, 1.0],
            leat: Vec::new(),
            course: Vec::new(),
            born: 0.0,
            ruined_at: f64::INFINITY,
            at: i,
        });
        let ponded = sim.migration_rate().to_vec();
        assert_eq!(sim.held[i], 0.0);
        let k = i - sim.reach_points(0.5 * sim.params.pond_length);
        assert!(
            ponded[k].abs() < free[k].abs() && sim.target[k] > 1.0 && sim.boost[k] > 1.0,
            "the pond above"
        );
        assert_eq!(ponded[i + 20], free[i + 20], "nothing below");
        sim.step_n(300);
        assert!(sim.mills[0].working(), "the channel keeps to the weir");
        let near = sim
            .centerline
            .iter()
            .map(|&c| dist(c, weir))
            .fold(f64::INFINITY, f64::min);
        assert!(near < 0.25 * w, "the weir holds its point: {near}");
    }

    /// A working mill on a growing sine, with its leat on the side `side`
    /// of the right normal. The bend above the weir migrates towards a
    /// leat on side 1 and away from one on side −1.
    fn mill_on_a_sine(side: f64) -> Sim {
        let mut sim = sine_sim(2.0, 40.0);
        let i = sim.centerline.len() / 2;
        let (up, down) = (sim.reach_points(LEAT_UP), sim.reach_points(LEAT_DOWN));
        let nrm = geom::right_normal(&sim.centerline, i);
        let course = sim.course(i - up, i + down, side);
        let (weir, across) = (sim.centerline[i], [side * nrm[0], side * nrm[1]]);
        let mut m = Mill {
            weir,
            across,
            leat: Vec::new(),
            course,
            born: 0.0,
            ruined_at: f64::INFINITY,
            at: i,
        };
        sim.rejoin(&mut m);
        sim.mills.push(m);
        sim
    }

    #[test]
    fn a_leat_follows_the_bank_and_is_lost_when_breached() {
        let mut sim = mill_on_a_sine(-1.0);
        let w = sim.params.width;
        let (course, intake) = (sim.mills[0].course.clone(), sim.mills[0].leat[0]);
        sim.step_n(300);
        let m = &sim.mills[0];
        assert!(
            m.working() && m.course == course,
            "the course stays where it was dug"
        );
        let up = sim.reach_points(LEAT_UP);
        for (end, k) in [
            (m.leat[0], m.at - up),
            (m.leat[m.leat.len() - 1], m.at - up + m.leat.len() - 1),
        ] {
            let gap = dist(end, sim.centerline[k]) - 0.5 * sim.widths[k] * w;
            assert!(gap.abs() < 0.05 * w, "the leat meets the bank: {gap}");
        }
        assert!(
            dist(m.leat[0], intake) > 0.5 * w,
            "the intake moves with the bank: {}",
            dist(m.leat[0], intake)
        );

        let mut breached = mill_on_a_sine(1.0);
        breached.step_n(300);
        assert!(
            !breached.mills[0].working(),
            "a bend that reaches the leat ruins the mill"
        );
    }

    #[test]
    fn holdings_found_mills_on_straight_reaches() {
        let mut sim = sine_sim(0.0, 40.0);
        sim.params.mill_rate = 1.0 / sim.params.dt;
        sim.land.owner.fill(0);
        sim.migration_rate();
        for _ in 0..20 {
            sim.found_mill();
        }
        let w = sim.params.width;
        assert!(!sim.mills.is_empty());
        for m in &sim.mills {
            let off = m
                .leat
                .iter()
                .map(|&c| (c[1] - m.weir[1]).abs())
                .fold(0.0, f64::max);
            assert!(
                (off - (0.5 + LEAT_OFFSET) * w).abs() < 0.1 * w,
                "the leat runs beside the bank: {off}"
            );
        }
        for (a, b) in sim.mills.iter().zip(sim.mills.iter().skip(1)) {
            assert!(dist(a.weir, b.weir) >= sim.params.mill_gap * w || sim.mills.len() < 2);
        }
        let mut bent = sine_sim(6.0, 12.0);
        (bent.params.mill_rate, bent.params.edge_margin) = (1.0 / bent.params.dt, 8.0);
        bent.land.owner.fill(0);
        for _ in 0..20 {
            bent.found_mill();
        }
        assert!(bent.mills.is_empty(), "no mill on a winding reach");
    }

    /// A channel along x with a bend 10 widths high between x = 110 and
    /// 116 widths, and an open lake running straight across the bend's
    /// mouth half a width below the channel, with its plugged ends beside
    /// the channel either side.
    fn bend_beside_a_lake() -> Sim {
        let mut sim = sine_sim(0.0, 40.0);
        (sim.params.cutoff_dist, sim.params.capture_ratio) = (1.0, 1.3);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        let at = |x: f64, y: f64| [x * w, mid + y * w];
        let mut pts: Vec<P> = (0..=20).map(|k| at(100.0 + 0.5 * k as f64, 0.0)).collect();
        pts.extend((1..=20).map(|k| at(110.0, 0.5 * k as f64)));
        pts.extend((1..=12).map(|k| at(110.0 + 0.5 * k as f64, 10.0)));
        pts.extend((1..=20).map(|k| at(116.0, 10.0 - 0.5 * k as f64)));
        pts.extend((1..=20).map(|k| at(116.0 + 0.5 * k as f64, 0.0)));
        sim.centerline = pts;
        sim.widths = vec![1.0; sim.centerline.len()];
        sim.line_version += 1;
        let lake: Vec<P> = (0..=30).map(|k| at(105.5 + 0.5 * k as f64, -0.5)).collect();
        sim.oxbows.push(Oxbow::lake(lake, vec![1.0; 31], 0.0, 0.0));
        sim
    }

    #[test]
    fn floods_cut_chutes_across_bare_bars_only() {
        // A bend 12 widths high and 4 across at its mouth, on bare sand or
        // on forest.
        let bend = |bare: bool| {
            let mut sim = sine_sim(0.0, 40.0);
            sim.params.chute_rate = 1e3;
            let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
            let at = |x: f64, y: f64| [x * w, mid + y * w];
            let mut pts: Vec<P> = (0..=200).map(|k| at(0.5 * k as f64, 0.0)).collect();
            pts.extend((1..=24).map(|k| at(100.0, 0.5 * k as f64)));
            pts.extend((1..=8).map(|k| at(100.0 + 0.5 * k as f64, 12.0)));
            pts.extend((1..=24).map(|k| at(104.0, 12.0 - 0.5 * k as f64)));
            pts.extend((1..=200).map(|k| at(104.0 + 0.5 * k as f64, 0.0)));
            sim.centerline = pts;
            sim.widths = vec![1.0; sim.centerline.len()];
            sim.line_version += 1;
            sim.erodibility
                .growth
                .fill(if bare { 0.0 } else { f32::INFINITY });
            let cut = (0..sim.centerline.len()).any(|_| sim.chute());
            (sim, cut)
        };
        let (sim, cut) = bend(true);
        assert!(cut && sim.chutes == 1 && sim.cutoffs == 1);
        let w = sim.params.width;
        let top = sim.bounds[1] / 2.0 + 6.0 * w;
        assert!(
            sim.centerline.iter().all(|q| q[1] < top),
            "the bend is cut off"
        );
        assert!(sim.oxbows[0].points.iter().any(|q| q[1] > top));
        assert!(!geom::self_intersects(&sim.centerline));
        assert!(!bend(false).1, "no chute across forest");
    }

    #[test]
    fn channel_takes_up_an_open_lake_across_a_bend() {
        let mut sim = bend_beside_a_lake();
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        let before = geom::length(&sim.centerline);
        assert!(sim.capture());
        assert_eq!((sim.captures, sim.cutoffs), (1, 1));
        assert_eq!(sim.widths.len(), sim.centerline.len());
        assert!(
            sim.centerline.iter().all(|q| q[1] < mid + w),
            "the channel leaves the bend"
        );
        assert!(geom::length(&sim.centerline) < before - 15.0 * w);
        assert!(!geom::self_intersects(&sim.centerline));
        // The bend is the new lake; the old lake's middle is now channel.
        let bend = sim.oxbows.last().unwrap();
        assert!(bend.born == sim.time && bend.points.iter().any(|q| q[1] > mid + 9.0 * w));
        let middle = [113.0 * w, mid - 0.5 * w];
        assert!(
            sim.oxbows
                .iter()
                .all(|o| o.points.iter().all(|&q| dist(q, middle) > 1.0))
        );
        assert!(sim.centerline.iter().any(|&q| dist(q, middle) < 1e-9));
    }

    #[test]
    fn a_plugged_lake_or_a_zero_ratio_does_not_capture() {
        let mut sim = bend_beside_a_lake();
        sim.oxbows[0].fill = CAPTURE_FILL;
        assert!(!sim.capture(), "the clay plug has closed the route");
        let mut sim = bend_beside_a_lake();
        sim.params.capture_ratio = 0.0;
        assert!(!sim.capture());
        // Round the bend is 26 widths, and through the lake 6.4.
        let mut sim = bend_beside_a_lake();
        sim.params.capture_ratio = 5.0;
        assert!(!sim.capture());
    }

    #[test]
    fn channel_alongside_an_oxbow_leaves_it_whole() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // Under the water on the channel side, but clear of the centreline.
        let beside: Vec<P> = (0..20)
            .map(|k| [(100.0 + 0.5 * k as f64) * w, mid + 0.3 * w])
            .collect();
        sim.oxbows
            .push(Oxbow::lake(beside.clone(), vec![1.0; 20], 0.0, 0.0));
        sim.step();
        assert_eq!(sim.oxbows.len(), 1);
        assert_eq!(sim.oxbows[0].points, beside);
    }
}
