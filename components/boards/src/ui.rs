use core::{
    ops::Range,
    sync::atomic::{AtomicBool, Ordering::Relaxed},
    time::Duration,
};

use trussed::{platform, types::ui};
use trussed_core::types::consent;

use buttons::UserPresence;
use rgb_led::{Intensities, RgbLed};

pub mod buttons;
pub mod rgb_led;

const BLACK: Intensities = Intensities {
    red: 0,
    green: 0,
    blue: 0,
};
const RED: Intensities = Intensities {
    red: u8::MAX,
    green: 0,
    blue: 0,
};
const TEAL: Intensities = Intensities {
    red: 0,
    green: u8::MAX,
    blue: 0x5a,
};
const WHITE: Intensities = Intensities {
    red: u8::MAX,
    green: u8::MAX,
    blue: u8::MAX,
};
// solokeys/solo2 idle/user-presence colours, used only by the Solo2 LED mapping.
#[cfg(feature = "board-solo2")]
const GREEN: Intensities = Intensities {
    red: 0,
    green: 0x3f,
    blue: 0,
};
#[cfg(feature = "board-solo2")]
const BLUE: Intensities = Intensities {
    red: 0,
    green: 0,
    blue: 0x7f,
};

static WAITING: AtomicBool = AtomicBool::new(false);

fn set_waiting(waiting: bool) {
    WAITING.store(waiting, Relaxed);
}

pub fn is_waiting() -> bool {
    WAITING.load(Relaxed)
}

pub trait Clock {
    fn uptime(&mut self) -> Duration;
}

pub struct UserInterface<C, P, L> {
    clock: C,
    buttons: Option<P>,
    rgb: Option<L>,
    status: Status,
    provisioner: bool,
}

impl<C: Clock, P: UserPresence, L: RgbLed> UserInterface<C, P, L> {
    pub fn new(mut clock: C, buttons: Option<P>, rgb: Option<L>) -> Self {
        let uptime = clock.uptime();
        let status = Status::Startup(uptime);
        let buttons = if cfg!(feature = "no-buttons") {
            None
        } else {
            buttons
        };
        let provisioner = cfg!(feature = "provisioner");

        let mut ui = Self {
            clock,
            buttons,
            status,
            rgb,
            provisioner,
        };
        ui.refresh_ui(uptime);
        ui
    }

    fn refresh_ui(&mut self, uptime: Duration) {
        if let Some(rgb) = &mut self.rgb {
            self.status.refresh(uptime);
            // Solo2: a live touch turns the LED blue, matching the solokeys
            // firmware, regardless of whether software is requesting presence.
            #[cfg(feature = "board-solo2")]
            {
                if let Some(buttons) = self.buttons.as_mut() {
                    if buttons.is_touched() {
                        let blue = LedMode::breathing(BLUE, Duration::from_secs(10), 4, 75);
                        rgb.set(blue.color(uptime));
                        return;
                    }
                }
            }
            let mode = self.status.led_mode(self.provisioner);
            rgb.set(mode.color(uptime));
        }
    }
}

impl<C: Clock, P: UserPresence, L: RgbLed> platform::UserInterface for UserInterface<C, P, L> {
    fn check_user_presence(&mut self) -> consent::Level {
        if let Some(buttons) = &mut self.buttons {
            set_waiting(true);
            let level = buttons.check_user_presence();
            set_waiting(false);
            level
        } else {
            consent::Level::Normal
        }
    }

    fn set_status(&mut self, status: ui::Status) {
        let uptime = self.uptime();
        self.status.update(status, uptime);
        self.refresh_ui(uptime);
    }

    fn refresh(&mut self) {
        let uptime = self.uptime();
        self.refresh_ui(uptime);
    }

    fn uptime(&mut self) -> Duration {
        self.clock.uptime()
    }

    fn wink(&mut self, duration: Duration) {
        let uptime = self.uptime();
        self.status = Status::Winking(uptime..uptime + duration);
        self.refresh_ui(uptime);
    }
}

pub struct CustomStatus(apps::CustomStatus);

impl CustomStatus {
    fn led_mode(&self, start: Duration) -> LedMode {
        let color = match self.0 {
            apps::CustomStatus::ReverseHotpSuccess => TEAL,
            apps::CustomStatus::ReverseHotpError => RED,
        };
        LedMode::simple_blinking(color, start)
    }

    fn allow_update(&self) -> bool {
        false
    }

    fn duration(&self) -> Option<Duration> {
        match self.0 {
            apps::CustomStatus::ReverseHotpSuccess => Some(Duration::from_secs(10)),
            apps::CustomStatus::ReverseHotpError => None,
        }
    }
}

impl From<apps::CustomStatus> for CustomStatus {
    fn from(status: apps::CustomStatus) -> Self {
        Self(status)
    }
}

impl TryFrom<u8> for CustomStatus {
    type Error = apps::UnknownStatusError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        apps::CustomStatus::try_from(value).map(From::from)
    }
}

pub enum Status {
    Startup(Duration),
    Idle,
    Processing,
    WaitingForUserPresence(Duration),
    Winking(Range<Duration>),
    Error,
    Custom {
        status: CustomStatus,
        start: Duration,
    },
}

impl Status {
    pub fn update(&mut self, status: ui::Status, uptime: Duration) {
        if status == ui::Status::Idle && matches!(self, Self::Startup(_) | Self::Winking(_)) {
            return;
        }
        if let Self::Custom { status, .. } = self {
            if !status.allow_update() {
                return;
            }
        }
        *self = (status, uptime).into();
    }

    pub fn refresh(&mut self, uptime: Duration) {
        let end = match self {
            Self::Startup(ref start) => Some(*start + Duration::from_millis(500)),
            Self::Winking(ref range) => Some(range.end),
            Self::Custom { status, start } => status.duration().map(|duration| *start + duration),
            _ => None,
        };
        if let Some(end) = end {
            if uptime > end {
                *self = Self::Idle;
            }
        }
    }

    pub fn led_mode(&self, is_provisioner: bool) -> LedMode {
        #[cfg(feature = "board-solo2")]
        {
            self.led_mode_solo2(is_provisioner)
        }
        #[cfg(not(feature = "board-solo2"))]
        {
            self.led_mode_default(is_provisioner)
        }
    }

    #[cfg(not(feature = "board-solo2"))]
    fn led_mode_default(&self, is_provisioner: bool) -> LedMode {
        match self {
            Self::Startup(_) => LedMode::constant(WHITE),
            Self::Idle => {
                if is_provisioner {
                    LedMode::constant(WHITE)
                } else {
                    LedMode::constant(BLACK)
                }
            }
            Self::Processing => LedMode::constant(TEAL),
            Self::WaitingForUserPresence(start) => LedMode::simple_blinking(WHITE, *start),
            Self::Error => LedMode::constant(RED),
            Self::Winking(range) => LedMode::simple_blinking(WHITE, range.start),
            Self::Custom { status, start } => status.led_mode(*start),
        }
    }

    // The solokeys/solo2 firmware shows a slow green idle heartbeat and a blue
    // user-presence indicator; mirror that feel on the Solo2 while leaving every
    // other board on the default mapping above.
    #[cfg(feature = "board-solo2")]
    fn led_mode_solo2(&self, is_provisioner: bool) -> LedMode {
        match self {
            // Constant green at boot instead of the default white flash, capped
            // at the idle heartbeat's peak brightness (GREEN scaled by the
            // breathe's max amplitude, 64/255) so it does not look brighter than
            // the heartbeat that follows.
            Self::Startup(_) => LedMode::constant(Intensities {
                red: 0,
                green: (GREEN.green as u16 * 64 / 255) as u8,
                blue: 0,
            }),
            Self::Idle => {
                if is_provisioner {
                    LedMode::constant(WHITE)
                } else {
                    LedMode::breathing(GREEN, Duration::from_secs(10), 4, 64)
                }
            }
            Self::Processing => LedMode::simple_blinking(GREEN, Duration::default()),
            Self::WaitingForUserPresence(_) => {
                LedMode::breathing(BLUE, Duration::from_secs(10), 4, 75)
            }
            Self::Error => LedMode::constant(RED),
            Self::Winking(range) => LedMode::simple_blinking(BLUE, range.start),
            Self::Custom { status, start } => status.led_mode(*start),
        }
    }
}

impl From<(ui::Status, Duration)> for Status {
    fn from((status, uptime): (ui::Status, Duration)) -> Self {
        match status {
            ui::Status::Idle => Self::Idle,
            ui::Status::Processing => Self::Processing,
            ui::Status::WaitingForUserPresence => Self::WaitingForUserPresence(uptime),
            ui::Status::Error => Self::Error,
            ui::Status::Custom(custom) => CustomStatus::try_from(custom)
                .map(|status| Self::Custom {
                    status,
                    start: uptime,
                })
                .unwrap_or_else(|_| {
                    error!("Unsupported custom UI status {}", custom);
                    Self::Error
                }),
            _ => {
                error!("Unsupported UI status {:?}", status);
                Self::Error
            }
        }
    }
}

pub enum LedMode {
    Constant {
        color: Intensities,
    },
    Blinking {
        on_color: Intensities,
        off_color: Intensities,
        period: Duration,
        start: Duration,
    },
    // Brightness ramps min..max..min over `period`, scaling `color` — the
    // solokeys/solo2 breathing heartbeat. Only the Solo2 mapping uses it.
    #[cfg(feature = "board-solo2")]
    Breathing {
        color: Intensities,
        period: Duration,
        min: u8,
        max: u8,
    },
}

impl LedMode {
    pub fn constant(color: Intensities) -> Self {
        Self::Constant { color }
    }

    pub fn blinking(
        on_color: Intensities,
        off_color: Intensities,
        period: Duration,
        start: Duration,
    ) -> Self {
        Self::Blinking {
            on_color,
            off_color,
            period,
            start,
        }
    }

    pub fn simple_blinking(color: Intensities, start: Duration) -> Self {
        Self::blinking(color, BLACK, Duration::from_millis(500), start)
    }

    #[cfg(feature = "board-solo2")]
    pub fn breathing(color: Intensities, period: Duration, min: u8, max: u8) -> Self {
        Self::Breathing {
            color,
            period,
            min,
            max,
        }
    }

    pub fn color(&self, uptime: Duration) -> Intensities {
        match self {
            Self::Constant { color } => *color,
            Self::Blinking {
                on_color,
                off_color,
                period,
                start,
            } => {
                let delta = (uptime - *start).as_millis() % period.as_millis();
                let is_on = delta < period.as_millis() / 2;
                if is_on {
                    *on_color
                } else {
                    *off_color
                }
            }
            #[cfg(feature = "board-solo2")]
            Self::Breathing {
                color,
                period,
                min,
                max,
            } => {
                // Exact port of solokeys `calculate_amplitude`: a |sin| breathing
                // wave. `period` is the sine-argument period, so the visible
                // breath cycle is half of it. amp = min + floor(|sin|*(max-min)),
                // then each channel is scaled by amp/255 (same as solo2's driver).
                let now = uptime.as_millis() as f32;
                let period_ms = (period.as_millis() as u32).max(1) as f32;
                let angle = core::f32::consts::TAU * now / period_ms;
                let amp = *min + (libm::fabsf(libm::sinf(angle)) * (*max - *min) as f32) as u8;
                let scale = |c: u8| ((c as u32 * amp as u32) / 255) as u8;
                Intensities {
                    red: scale(color.red),
                    green: scale(color.green),
                    blue: scale(color.blue),
                }
            }
        }
    }
}
