use apdu_dispatch::interchanges::{
    Channel as CcidChannel, Requester as CcidRequester, Responder as CcidResponder,
};
use apps::InitStatus;
use boards::{
    flash::ExtFlashStorage,
    init::{self, UsbNfc},
    solo2::{
        button::ThreeButtons,
        led::RgbLed,
        nfc::{self, NfcChip},
        prince,
        spi::{self, Spi, SpiConfig},
        ButtonsTimer, InternalFlashStorage, SOLO2, PwmTimer,
    },
    soc::{
        lpc55::{clock_controller::DynamicClockController, Lpc55},
        Soc,
    },
    store::{self, RunnerStore},
    ui::{
        buttons::{self, Press},
        rgb_led::RgbLed as _,
        UserInterface,
    },
    Apps, Trussed,
};
use embedded_hal::timer::{Cancel, CountDown};
use hal::{
    drivers::{
        clocks,
        flash::FlashGordon,
        pins::{self, direction},
        Timer,
    },
    peripherals::{
        ctimer::{self, Ctimer},
        flexcomm::Flexcomm0,
        inputmux::InputMux,
        pfr::Pfr,
        pint::Pint,
        prince::Prince,
        rng::Rng,
        usbhs::Usbhs,
    },
    time::{DurationExtensions as _, RateExtensions as _},
    typestates::{
        init_state::{Enabled, Unknown},
        pin::state::Gpio,
    },
    Pin,
};
use lpc55_hal as hal;
use nfc_device::Iso14443;

pub struct All {
    pub status: InitStatus,
    pub store: RunnerStore<SOLO2>,
    pub nfc: Option<Iso14443<NfcChip>>,
    pub usb_classes: Option<UsbClasses>,
    pub rgb: RgbLed,
    pub buttons: ThreeButtons,
    pub clock_controller: DynamicClockController,
}

pub fn init(
    device_peripherals: lpc55_hal::raw::Peripherals,
    core_peripherals: rtic::export::Peripherals,
) -> All {
    let hal = lpc55_hal::Peripherals::from((device_peripherals, core_peripherals));

    let require_prince = cfg!(not(feature = "no-encrypted-storage"));
    let secure_firmware_version = None; // Solo2 no usa secure firmware version
    let nfc_enabled = true;
    let boot_to_bootrom = true;

    let init_result = init::start(hal.syscon, hal.pmc, hal.anactrl)
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
        .next(hal.usbhs);

    All {
        status: init_result.status,
        store: init_result.store,
        nfc: init_result.nfc,
        usb_classes: init_result.usb_classes,
        rgb: init_result.rgb,
        buttons: init_result.buttons,
        clock_controller: init_result.clock_controller,
    }
}

pub struct Stage2 {
    status: InitStatus,
    peripherals: Peripherals,
    clocks: Clocks,
    basic: Basic,
}

impl Stage2 {
    fn setup_spi(&mut self, flexcomm0: Flexcomm0<Unknown>, config: SpiConfig) -> Spi {
        let token = self.clocks.clocks.support_flexcomm_token().unwrap();
        let spi = flexcomm0.enabled_as_spi(&mut self.peripherals.syscon, &token);
        spi::init(spi, &mut self.clocks.iocon, config)
    }

    fn setup_fm11nc08(
        &mut self,
        spi: Spi,
        inputmux: InputMux<Unknown>,
        pint: Pint<Unknown>,
        nfc_rq: CcidRequester<'static>,
    ) -> Option<Iso14443<NfcChip>> {
        let mut mux = inputmux.enabled(&mut self.peripherals.syscon);
        let mut pint = pint.enabled(&mut self.peripherals.syscon);
        let nfc_irq = self.clocks.nfc_irq.take().unwrap();
        pint.enable_interrupt(
            &mut mux,
            &nfc_irq,
            lpc55_hal::peripherals::pint::Slot::Slot0,
            lpc55_hal::peripherals::pint::Mode::ActiveLow,
        );
        mux.disabled(&mut self.peripherals.syscon);

        let nfc = nfc::try_setup(
            spi,
            &mut self.clocks.gpio,
            &mut self.clocks.iocon,
            nfc_irq,
            &mut self.basic.timer,
            true,
            &mut self.status,
        )?;

        Some(Iso14443::new(nfc, nfc_rq))
    }

    #[inline(never)]
    pub fn next(
        mut self,
        flexcomm0: Flexcomm0<Unknown>,
        flexcomm5: Flexcomm5<Unknown>,
        mux: InputMux<Unknown>,
        pint: Pint<Unknown>,
        nfc_enabled: bool,
    ) -> Stage3 {
        static NFC_CHANNEL: CcidChannel = Channel::new();
        let (nfc_rq, _nfc_rp) = NFC_CHANNEL.split().unwrap();

        let nfc = if nfc_enabled {
            let spi = self.setup_spi(flexcomm0, SpiConfig::Nfc);
            self.setup_fm11nc08(spi, mux, pint, nfc_rq)
        } else {
            None
        };

        Stage3 {
            status: self.status,
            peripherals: self.peripherals,
            clocks: self.clocks,
            basic: self.basic,
            nfc,
        }
    }
}

pub struct Stage3 {
    status: InitStatus,
    peripherals: Peripherals,
    clocks: Clocks,
    basic: Basic,
    nfc: Option<Iso14443<NfcChip>>,
}

impl Stage3 {
    pub fn next(
        mut self,
        rng: Rng<Unknown>,
        prince: Prince<Unknown>,
        flash: FlashGordon<Unknown>,
    ) -> Stage4 {
        let rng = rng.enabled(&mut self.peripherals.syscon);
        let prince = prince.enabled(&mut self.peripherals.syscon);
        let flash = flash.enabled(&mut self.peripherals.syscon);

        Stage4 {
            status: self.status,
            peripherals: self.peripherals,
            clocks: self.clocks,
            basic: self.basic,
            nfc: self.nfc,
            rng,
            prince,
            flash,
        }
    }
}

pub struct Stage4 {
    status: InitStatus,
    peripherals: Peripherals,
    clocks: Clocks,
    basic: Basic,
    nfc: Option<Iso14443<NfcChip>>,
    rng: Rng<Enabled>,
    prince: Prince<Enabled>,
    flash: FlashGordon<Enabled>,
}

impl Stage4 {
    pub fn next(mut self) -> Stage5 {
        if cfg!(not(feature = "no-encrypted-storage")) {
            prince::enable(&mut self.prince);
        }

        Stage5 {
            status: self.status,
            peripherals: self.peripherals,
            clocks: self.clocks,
            basic: self.basic,
            nfc: self.nfc,
            rng: self.rng,
            prince: self.prince,
            flash: self.flash,
        }
    }
}
