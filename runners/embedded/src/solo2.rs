pub mod init;

use boards::solo2::SOLO2;

use crate::{VERSION, VERSION_STRING};

pub fn init(
    device_peripherals: lpc55_hal::raw::Peripherals,
    core_peripherals: rtic::export::Peripherals,
) -> init::All {
    boards::init::init_logger::<SOLO2>(VERSION_STRING);

    let hal = lpc55_hal::Peripherals::from((device_peripherals, core_peripherals));

    let require_prince = cfg!(not(feature = "no-encrypted-storage"));
    let secure_firmware_version = None;
    let nfc_enabled = true;
    let boot_to_bootrom = true;

    init::start(hal.syscon, hal.pmc, hal.anactrl)
        .next(hal.iocon, hal.gpio)
        .next(
            hal.adc,
            hal.ctimer.0,
            hal.ctimer.1,
            hal.ctimer.2,
            hal.ctimer.3,
            hal.ctimer.4,
            hal.pfr,
            secure_firmware_version,
            require_prince,
            boot_to_bootrom,
        )
        .next(
            hal.flexcomm.0,
            hal.flexcomm.5,
            hal.inputmux,
            hal.pint,
            nfc_enabled,
        )
        .next(hal.rng, hal.prince, hal.flash)
        .next()
        .next(hal.rtc)
        .next(hal.usbhs)
}
